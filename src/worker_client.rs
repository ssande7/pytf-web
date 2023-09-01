use std::{time::{Instant, Duration}, ops::{DerefMut, Deref}, sync::{Mutex, Arc, Condvar}};

use actix::{prelude::*, io::{SinkWrite, WriteHandler}};
use actix_codec::Framed;
use anyhow::anyhow;
use awc::{BoxedSocket, ws, error::WsProtocolError};
use futures_util::{stream::StreamExt, Future};
use futures::stream::{SplitSink, SplitStream};

use crate::{
    authentication::UserCredentials,
    pytf_runner::{ PytfRunner, PytfStop, PytfPauseFiles, PytfCycle },
    pytf_frame::SegmentProcessor,
    pytf_config::PytfConfig,
    split_nullterm_utf8_str,
};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

const SERVER_TIMEOUT: Duration = Duration::from_secs(90);


/// To worker
pub const JOB_HEADER:     &[u8] = b"job\0";
pub const STEAL_HEADER:   &[u8] = b"steal\0";

/// Bidirectional
pub const PAUSE_HEADER:   &[u8] = b"pause\0";

/// From worker
pub const SEGMENT_HEADER: &[u8] = b"seg\0";
pub const DONE_HEADER:    &[u8] = b"done\0";
pub const FAILED_HEADER:  &[u8] = b"fail\0";

type WsFramedSink = SplitSink<Framed<BoxedSocket, ws::Codec>, ws::Message>;
type WsFramedStream = SplitStream<Framed<BoxedSocket, ws::Codec>>;

pub struct PytfServer {
    socket_sink: SinkWrite<ws::Message, WsFramedSink>,
    heartbeat: Instant,
    worker: Option<Addr<PytfRunner>>,
    segment_proc: Addr<SegmentProcessor>,
}

impl PytfServer {
    /// Initialise a web socket client. Panics if the client
    /// fails to initialise, or the server cannot return a test ping.
    pub async fn connect<S: AsRef<str>>(server_addr: S, key: S) -> anyhow::Result<Addr<Self>> {
        // Log in to server to allow web socket connection
        let login = match awc::Client::new()
            .post(format!("http://{}/login", server_addr.as_ref()))
            .send_json(&UserCredentials {
                username: "worker".into(),
                password: key.as_ref().to_owned()
            }).await
        {
            Ok(login) => login,
            Err(e) => return Err(anyhow!("{e}")),
        };

        // Get ID cookie
        let Some(login_id) = login.cookie("id") else {
            return Err(anyhow!("Login failed: Didn't receive id cookie."))
        };


        // Connect to web socket
        let socket = match awc::Client::new()
            .ws(format!("ws://{}/socket", server_addr.as_ref()))
            .cookie(login_id)
            .connect()
            .await
        {
            Ok((_, socket)) => socket,
            Err(e) => return Err(anyhow!("Error connecting to web socket: {e}")),
        };

        let (sink, stream): (WsFramedSink, WsFramedStream) = socket.split();
        Ok(Self::create(|ctx| {
            ctx.add_stream(stream);
            let addr = ctx.address();
            Self {
                socket_sink: SinkWrite::new(sink, ctx),
                heartbeat: Instant::now(),
                worker: None,
                segment_proc: spawn_on_arbiter(async move {
                    SegmentProcessor::new(addr).start()
                }).expect("Failed to spawn segment processor thread"),
            }
        }))
    }

    fn heartbeat(&self, ctx: &mut Context<Self>) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.heartbeat) > SERVER_TIMEOUT {
                println!("Lost connection to server");
                ctx.stop();
                return;
            }
            ctx.address().do_send(WsMessage(ws::Message::Ping("".into())));
        });
    }

    /// Create a new `PytfRunner` worker in a new thread and tell it to begin cycling.
    /// Returns the previous worker if there was one and if the worker started successfully.
    fn start_worker(&mut self, config: PytfConfig, addr: Addr<Self>) -> anyhow::Result<Option<Addr<PytfRunner>>> {
        let runner = PytfRunner::new(config, addr, self.segment_proc.clone())?;
        if let Some(worker) = spawn_on_arbiter(async move {
            let worker = runner.start();
            worker.do_send(PytfCycle {});
            worker
        }) {
            Ok(self.worker.replace(worker))
        } else {
            Err(anyhow!("Failed to spawn worker"))
        }
    }
}

/// Create a new arbiter, execute a future on it to spawn a new actor,
/// and return that actor's address
fn spawn_on_arbiter<A: Actor, Fut>(func: Fut) -> Option<Addr<A>>
where Fut: Future<Output = Addr<A>> + Send + 'static {
    let signal = Arc::new((
        Mutex::<Option<Addr<A>>>::new(None),
        Condvar::new()
    ));
    let spawn_signal = signal.clone();
    let lock = signal.0.lock().unwrap();
    if Arbiter::new().spawn(async move {
        let addr = func.await;
        { *spawn_signal.0.lock().unwrap() = Some(addr); }
        spawn_signal.1.notify_one();
    }) {
        signal.1.wait(lock).unwrap().clone()
    } else { None }
}

#[derive(Message)]
#[rtype(result="()")]
struct NewWorker { worker: Addr<PytfRunner> }
struct StartHandler {
    worker: Option<Addr<PytfRunner>>,
}
impl Actor for StartHandler {
    type Context = Context<Self>;
}
impl Handler<NewWorker> for StartHandler {
    type Result = ();
    fn handle(&mut self, msg: NewWorker, ctx: &mut Self::Context) -> Self::Result {
        self.worker = Some(msg.worker);
    }
}



/// Convenience wrapper around web socket message to be forwarded on to main server
#[derive(Message)]
#[rtype(result="()")]
pub struct WsMessage(pub ws::Message);
impl From<ws::Message> for WsMessage {
    fn from(value: ws::Message) -> Self {
        Self(value)
    }
}
impl Deref for WsMessage {
    type Target = ws::Message;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for WsMessage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Handler<WsMessage> for PytfServer {
    type Result = ();
    fn handle(&mut self, msg: WsMessage, _ctx: &mut Self::Context) -> Self::Result {
        if let Err(e) = self.socket_sink.write(msg.0) {
            eprintln!("WARNING: Failed to send message: {e:?}")
        }
    }
}

impl WriteHandler<WsProtocolError> for PytfServer {}

impl StreamHandler<Result<ws::Frame, awc::error::WsProtocolError>> for PytfServer {
    fn handle(&mut self, msg: Result<ws::Frame, awc::error::WsProtocolError>, ctx: &mut Self::Context) {
        let msg = match msg {
            Err(e) => {
                println!("Received erroneous message: {e}");
                ctx.stop();
                return;
            }
            Ok(msg) => msg,
        };
        println!("Worker received message: {msg:?}");
        match msg {
            ws::Frame::Text(_) => (),
            ws::Frame::Binary(mut bytes) => {
                if bytes.starts_with(JOB_HEADER) {
                    let _ = bytes.split_to(JOB_HEADER.len());
                    let config = match std::str::from_utf8(bytes.as_ref()) {
                        Ok(config) => config,
                        Err(e) => {
                            eprintln!("Failed to deserialize config of new job: {e}");
                            return
                        }
                    };
                    println!("Config string: {config}");
                    let config: PytfConfig = match serde_json::from_str(config) {
                        Ok(config) => config,
                        Err(e) => {
                            eprintln!("Failed to deserialize config for new job: {e}");
                            return
                        }
                    };
                    let jobname = config.name.clone();
                    match self.start_worker(config, ctx.address()) {
                        Ok(Some(old_worker)) => {
                            old_worker.do_send(PytfStop { jobname: None })
                        },
                        Ok(None) => (),
                        Err(e) => {
                            eprintln!("Failed to start new job {jobname}: {e}");
                            let _ = self.socket_sink.write(ws::Message::Binary(
                                [FAILED_HEADER, jobname.as_bytes()].concat().into()));
                            return
                        }
                    }
                } else if bytes.starts_with(PAUSE_HEADER) {
                    // Got signal to pause current job
                    let _ = bytes.split_to(PAUSE_HEADER.len());
                    let Ok(jobname) = std::str::from_utf8(bytes.as_ref())
                    else {
                        eprintln!("Received invalid string for jobname to stop");
                        return
                    };
                    if let Some(worker) = &self.worker {
                        worker.do_send(PytfStop { jobname: Some(jobname.to_owned()) });
                    } else {
                        println!("Received stop signal for job \"{jobname}\", but no worker running");
                        return
                    }
                } else if bytes.starts_with(STEAL_HEADER) {
                    // Got existing job to continue from
                    let _ = bytes.split_to(STEAL_HEADER.len());
                    let config = match split_nullterm_utf8_str(&mut bytes) {
                        Ok(config) => config,
                        Err(e) => {
                            eprintln!("Failed to read config information from job to resume: {e}");
                            return
                        }
                    };
                    let config: PytfConfig = match serde_json::from_str(&config) {
                        Ok(config) => config,
                        Err(e) => {
                            eprintln!("Failed to deserialize config for job to resume: {e}");
                            return
                        }
                    };
                    let pause_data = match PytfPauseFiles::unpack(bytes.as_ref()) {
                        Ok(data) => data,
                        Err(e) => {
                            eprintln!("Error decoding pause data: {e}");
                            return
                        }
                    };
                    if let Err(e) = pause_data.to_disk(&config.work_directory, &config.name) {
                        eprintln!("Failed to write job data to disk: {e}");
                        let _ = self.socket_sink.write(ws::Message::Binary(
                            [FAILED_HEADER, config.name.as_bytes()].concat().into()));
                        return
                    };
                    let jobname = config.name.clone();
                    match self.start_worker(config, ctx.address()) {
                        Ok(Some(old_worker)) => old_worker.do_send(PytfStop { jobname: None }),
                        Ok(None) => (),
                        Err(e) => {
                            eprintln!("Failed to resume job {jobname}: {e}");
                            let _ = self.socket_sink.write(ws::Message::Binary(
                                [FAILED_HEADER, jobname.as_bytes()].concat().into()));
                            return
                        }
                    }
                } else {
                    println!("Received unknown message.");
                }
            },
            ws::Frame::Ping(ping) => {
                println!("Received ping {ping:?}");
                self.heartbeat = Instant::now();
                let _ = self.socket_sink.write(ws::Message::Pong(ping));
            }
            ws::Frame::Pong(_) => { self.heartbeat = Instant::now() },
            ws::Frame::Close(_) => ctx.stop(),
            ws::Frame::Continuation(_) => (),
        }
    }
}

impl Actor for PytfServer {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        println!("Starting heartbeat");
        self.heartbeat(ctx);
    }

    /// Connection to main server is shutting down
    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        println!("Shutting down connection to server");
        // Cancel the worker if it's running.
        // NOTE: This will lose any pause data since the packet won't be forwarded.
        if let Some(worker) = &self.worker {
            worker.send(PytfStop { jobname: None })
                .into_actor(self)
                .then(|_, _, _| {fut::ready(())})
                .wait(ctx);
        }
        Running::Stop
    }
}
