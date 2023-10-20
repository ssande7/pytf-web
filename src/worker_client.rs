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
    pytf_frame::{SegmentProcessor, NewSocket, WS_FRAME_SIZE_LIMIT},
    pytf_config::PytfConfig,
    split_nullterm_utf8_str,
};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

const SERVER_TIMEOUT: Duration = Duration::from_secs(90);
const RECONNECT_TIMER: Duration = Duration::from_secs(60);


/// To worker
pub const JOB_HEADER:     &[u8] = b"job\0";
pub const STEAL_HEADER:   &[u8] = b"steal\0";

/// Bidirectional
pub const PAUSE_HEADER:   &[u8] = b"pause\0";

/// From worker
pub const SEGMENT_HEADER: &[u8] = b"seg\0";
pub const DONE_HEADER:    &[u8] = b"done\0";
pub const FAILED_HEADER:  &[u8] = b"fail\0";
pub const RESUME_HEADER:  &[u8] = b"resume\0";

type WsFramedSink = SplitSink<Framed<BoxedSocket, ws::Codec>, ws::Message>;
type WsFramedStream = SplitStream<Framed<BoxedSocket, ws::Codec>>;

pub struct PytfServer {
    server_addr: String,
    key: String,
    socket_sink: SinkWrite<ws::Message, WsFramedSink>,
    heartbeat: Instant,
    worker: Option<Addr<PytfRunner>>,
    segment_proc: Addr<SegmentProcessor>,
}

async fn open_ws_connection(server_addr: String, key: String) -> anyhow::Result<(WsFramedSink, WsFramedStream)> {
    // Log in to server to allow web socket connection
    let login = match awc::Client::new()
        .post(format!("http://{}/login", server_addr))
        .send_json(&UserCredentials {
            username: "worker".into(),
            password: key.clone(),
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
        .ws(format!("ws://{}/socket", server_addr))
        .cookie(login_id)
        .max_frame_size(WS_FRAME_SIZE_LIMIT)
        .connect()
        .await
        {
            Ok((_, socket)) => socket,
            Err(e) => return Err(anyhow!("Error connecting to web socket: {e}")),
        };
    Ok(socket.split())
}

#[async_recursion::async_recursion(?Send)]
async fn delay_and_reconnect(server_addr: String, key: String) -> (WsFramedSink, WsFramedStream) {
    log::debug!("Waiting to try reconnection");
    actix_rt::time::sleep(RECONNECT_TIMER).await;
    match open_ws_connection(server_addr.clone(), key.clone()).await {
        Ok(connection) => {
            log::info!("Reconnected.");
            connection
        }
        _ => {
            log::warn!("Failed to reconnect! Trying again in {}s...", RECONNECT_TIMER.as_secs());
            delay_and_reconnect(server_addr, key).await
        }
    }
}


impl PytfServer {
    /// Initialise a web socket client. Waits and attempts reconnection
    /// if client initialisation fails or the server can't return a test ping.
    pub async fn connect(server_addr: String, key: String) -> Addr<Self> {
        let (sink, stream) = match open_ws_connection(server_addr.clone(), key.clone()).await {
            Ok(connection) => connection,
            Err(e) => {
                log::warn!("Error while connecting to server: {e}");
                delay_and_reconnect(server_addr.clone(), key.clone()).await
            }
        };
        Self::create(|ctx| {
            ctx.add_stream(stream);
            let addr = ctx.address();
            Self {
                server_addr, key,
                socket_sink: SinkWrite::new(sink, ctx),
                heartbeat: Instant::now(),
                worker: None,
                segment_proc: spawn_on_arbiter(async move {
                    SegmentProcessor::new(addr).start()
                }).expect("Failed to spawn segment processor thread"),
            }
        })
    }

    fn heartbeat(&self, ctx: &mut Context<Self>) {
        ctx.run_interval(HEARTBEAT_INTERVAL, move |act, ctx| {
            if Instant::now().duration_since(act.heartbeat) > SERVER_TIMEOUT {
                log::warn!("Lost connection to server. Attempting to reconnect...");
                act.socket_sink.close();
                delay_and_reconnect(act.server_addr.clone(), act.key.clone())
                    .into_actor(act)
                    .then(|(sink, socket), act, ctx| {
                        act.socket_sink = SinkWrite::new(sink, ctx);
                        ctx.add_stream(socket);
                        ctx.address().do_send(WsMessage(ws::Message::Ping("".into())));
                        fut::ready(())
                    })
                    .wait(ctx);
                return;
            }
            ctx.address().do_send(WsMessage(ws::Message::Ping("".into())));
        });
    }

    /// Create a new `PytfRunner` worker in a new thread and tell it to begin cycling.
    /// Returns the previous worker if there was one and if the worker started successfully.
    fn start_worker(&mut self, config: PytfConfig, addr: Addr<Self>, resuming: bool) -> anyhow::Result<Option<Addr<PytfRunner>>> {
        let runner = PytfRunner::new(config, addr, self.segment_proc.clone(), resuming)?;
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
    fn handle(&mut self, msg: NewWorker, _ctx: &mut Self::Context) -> Self::Result {
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
            let display = format!("{e:?}");
            if display.len() > 10000 {
                log::warn!("Failed to send message: {}\n...\n{}",
                    display.chars().take(300).collect::<String>(),
                    display.chars().skip(display.chars().count() - 200).collect::<String>()
                );
            } else {
                log::warn!("Failed to send message: {e:?}")
            }
        }
    }
}

impl WriteHandler<WsProtocolError> for PytfServer {}

impl StreamHandler<Result<ws::Frame, awc::error::WsProtocolError>> for PytfServer {
    fn handle(&mut self, msg: Result<ws::Frame, awc::error::WsProtocolError>, ctx: &mut Self::Context) {
        let msg = match msg {
            Err(e) => {
                log::error!("Received erroneous message: {e}");
                ctx.stop();
                return;
            }
            Ok(msg) => msg,
        };
        match msg {
            ws::Frame::Text(_) => (),
            ws::Frame::Binary(mut bytes) => {
                if bytes.starts_with(JOB_HEADER) {
                    let _ = bytes.split_to(JOB_HEADER.len());
                    let config = match std::str::from_utf8(bytes.as_ref()) {
                        Ok(config) => config,
                        Err(e) => {
                            log::error!("Failed to deserialize config of new job: {e}");
                            return
                        }
                    };
                    log::debug!("Config string: {config}");
                    let config: PytfConfig = match serde_json::from_str(config) {
                        Ok(config) => config,
                        Err(e) => {
                            log::error!("Failed to deserialize config for new job: {e}");
                            return
                        }
                    };
                    let jobname = config.name.clone();
                    match self.start_worker(config, ctx.address(), false) {
                        Ok(Some(old_worker)) => {
                            old_worker.do_send(PytfStop { jobname: None })
                        },
                        Ok(None) => (),
                        Err(e) => {
                            log::error!("Failed to start new job {jobname}: {e}");
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
                        log::error!("Received invalid string for jobname to stop");
                        return
                    };
                    if let Some(worker) = &self.worker {
                        worker.do_send(PytfStop { jobname: Some(jobname.to_owned()) });
                    } else {
                        log::warn!("Received stop signal for job \"{jobname}\", but no worker running");
                        return
                    }
                } else if bytes.starts_with(STEAL_HEADER) {
                    // Got existing job to continue from
                    let _ = bytes.split_to(STEAL_HEADER.len());
                    let config = match split_nullterm_utf8_str(&mut bytes) {
                        Ok(config) => config,
                        Err(e) => {
                            log::error!("Failed to read config information from job to resume: {e}");
                            return
                        }
                    };
                    let config: PytfConfig = match serde_json::from_str(&config) {
                        Ok(config) => config,
                        Err(e) => {
                            log::error!("Failed to deserialize config for job to resume: {e}");
                            return
                        }
                    };
                    let pause_data = match PytfPauseFiles::unpack(bytes.as_ref()) {
                        Ok(data) => data,
                        Err(e) => {
                            log::error!("Error decoding pause data: {e}");
                            return
                        }
                    };
                    if let Err(e) = pause_data.to_disk(&config.work_directory, &config.name) {
                        log::error!("Failed to write job data to disk: {e}");
                        let _ = self.socket_sink.write(ws::Message::Binary(
                            [FAILED_HEADER, config.name.as_bytes()].concat().into()));
                        return
                    };
                    let jobname = config.name.clone();
                    match self.start_worker(config, ctx.address(), true) {
                        Ok(Some(old_worker)) => {
                            old_worker.do_send(PytfStop { jobname: None });
                            let _ = self.socket_sink.write(ws::Message::Binary(
                                [RESUME_HEADER, jobname.as_bytes()].concat().into()));
                        },
                        Ok(None) => (),
                        Err(e) => {
                            log::error!("Failed to resume job {jobname}: {e}");
                            let _ = self.socket_sink.write(ws::Message::Binary(
                                [FAILED_HEADER, jobname.as_bytes()].concat().into()));
                            return
                        }
                    }
                } else {
                    log::warn!("Received unknown message.");
                }
            },
            ws::Frame::Ping(ping) => {
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
        log::debug!("Starting heartbeat");
        self.heartbeat(ctx);
    }

    /// Connection to main server is shutting down
    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        if !self.socket_sink.closed() {
            log::debug!("Setting up reconnection");
            self.socket_sink.close();
            delay_and_reconnect(self.server_addr.clone(), self.key.clone())
                .into_actor(self)
                .then(move |(sink, stream), act, _ctx| {
                    log::debug!("Creating new socket.");
                    // TODO: make worker and segment_proc retry sending
                    // failed messages somehow?
                    // Maybe shouldn't preserve saved messages if server is a new instance?
                    let worker = act.worker.take();
                    let addr = Self::create(|ctx| {
                            ctx.add_stream(stream);
                            Self {
                                server_addr: act.server_addr.clone(),
                                key: act.key.clone(),
                                socket_sink: SinkWrite::new(sink, ctx),
                                heartbeat: Instant::now(),
                                worker: worker.clone(),
                                segment_proc: act.segment_proc.clone(),
                            }
                        });
                    if let Some(worker) = &worker {
                        worker.do_send(PytfStop { jobname: None });
                    }
                    act.segment_proc.do_send(NewSocket { addr });

                    fut::ready(())
                })
                .wait(ctx);
            return Running::Continue;
        }

        log::info!("Shutting down connection to server");
        Running::Stop
    }
}
