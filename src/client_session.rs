use std::{time::{Duration, Instant}, sync::Arc};

use actix::prelude::*;
use actix_web_actors::ws;
use pytf_web::pytf_config::PytfConfig;

use crate::job_queue::{Job, JobServer, ClientConnect, ClientDisconnect, ClientReqJob};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct ClientWsSession {
    pub id: Arc<String>,

    /// Set true when received a `ClientForceDisconnect` message from server to
    /// avoid sending `ClientDisconnect` message back to server when this Actor stops.
    force_disconnect: bool,

    pub heartbeat: Instant,

    pub job: Option<Job>,

    pub job_server: Addr<JobServer>,
}

impl ClientWsSession {
    pub fn new(id: String, job_server: Addr<JobServer>) -> Self {
        Self {
            id: Arc::new(id),
            force_disconnect: false,
            heartbeat: Instant::now(),
            job: None,
            job_server,
        }
    }

    fn heartbeat(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.heartbeat) > CLIENT_TIMEOUT {
                println!("Lost connection to client {}", act.id);
                // act.job_server.do_send(ClientDisconnect { id: act.id.clone() });
                ctx.stop();
                return;
            }
            ctx.ping(b"");
        });
    }
}


impl Actor for ClientWsSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.heartbeat(ctx);

        let addr = ctx.address();
        self.job_server
            .send(ClientConnect {
                id: self.id.clone(),
                addr,
            })
            .into_actor(self)
            .then(|res, _act, ctx| {
                match res {
                    Ok(_) => (),
                    _ => ctx.stop(), // Something went wrong
                }
                fut::ready(())
            })
            .wait(ctx);
    }

    fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
        println!("Sending disconnect signal for client {}", self.id);
        if !self.force_disconnect {
            self.job_server.do_send(ClientDisconnect { id: self.id.clone() });
        }
        Running::Stop
    }
}

#[derive(Message)]
#[rtype(result="()")]
pub struct ClientForceDisconnect {}

impl Handler<ClientForceDisconnect> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, msg: ClientForceDisconnect, ctx: &mut Self::Context) -> Self::Result {
        // TODO: Send a disconnect message to client?

        // Set my id to null before calling ctx.stop() since we got this message because job_server
        // received a new connection with my id, and therefore it shouldn't be cancelling jobs
        // attached to that id when I disconnect.
        self.force_disconnect = true;
        ctx.stop();
    }
}


// Incoming stream from client
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for ClientWsSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        let msg = match msg {
            Err(_) => {
                ctx.stop();
                return;
            }
            Ok(msg) => msg,
        };

        println!("Client session received message: {msg:?}");
        match msg {
            ws::Message::Ping(msg) => {
                self.heartbeat = Instant::now();
                ctx.pong(&msg);
            }
            ws::Message::Pong(_) => {
                self.heartbeat = Instant::now();
            }
            ws::Message::Text(text) => {
                let text = text.trim();
                if let Ok(mut config) = serde_json::from_str::<PytfConfig>(&text) {
                    config.canonicalize();
                    config.prefill();
                    self.job_server.send(ClientReqJob {
                        config,
                        client_id: self.id.clone(),
                        client_addr: ctx.address(),
                        client_prev_job: self.job.clone(),
                    })
                    .into_actor(self)
                    .then(|res, act, ctx| {
                        match res {
                            Ok(job) => act.job = Some(job),
                            _ => ctx.stop(), // Something went wrong
                        }
                        fut::ready(())
                    })
                    .wait(ctx);
                } else if text == "cancel" {
                    println!("Received cancel signal for client {}", self.id);
                    self.job = None;
                    if let Some(job) = &self.job {
                        job.write().unwrap().remove_client(&self.id);
                    }
                    println!("Done processing cancel for client {}", self.id);
                } else if let Ok(frame_id) = text.parse::<usize>() {
                    // Client has successfully received frames up to `frame_id`,
                    // so check if we have more for them
                    if let Some(job) = &self.job {
                        if frame_id < job.read().unwrap().frames_available {
                            // TODO: Send next frame
                            // ctx.binary(frame);
                        }
                    }
                }
            }
            ws::Message::Binary(_) => println!("Unexpected binary from client {}", self.id),
            ws::Message::Close(reason) => {
                ctx.close(reason);
                ctx.stop();
            }
            ws::Message::Continuation(_) => {
                ctx.stop();
            }
            ws::Message::Nop => (),
        }
    }
}

#[derive(Message)]
#[rtype(result="()")]
pub struct TrajectoryPing {}

// Trajectory data to send back to client
impl Handler<TrajectoryPing> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, msg: TrajectoryPing, ctx: &mut Self::Context) -> Self::Result {
        // TODO: Check job for new data to stream out
        ctx.text("new_frames"); // Notify client of new frames available
    }
}
