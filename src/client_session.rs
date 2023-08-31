use std::{time::{Duration, Instant}, sync::Arc};

use actix::prelude::*;
use actix_web_actors::ws;
use pytf_web::pytf_config::PytfConfigMinimal;

use crate::job_queue::{Job, JobServer, ClientConnect, ClientDisconnect, ClientReqJob, AssignJobs};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

/** MESSAGES TO CLIENT
* text("failed") => Job has failed - try a different configuration.
*
* text("done") => Job has completed successfully.
*
* binary(b"{frame id: u32 little endian}{frame data}") => Frame of current job
*
*/

const MSG_JOB_FAILED: &str = "failed";
const MSG_JOB_DONE:   &str = "done";

/** MESSAGES FROM CLIENT
* text("cancel") => Cancel the current job
*
* text("{PytfConfigMinimal as json}") => New configuration to run
*
* text("{segment_id, parseable to usize}") => Requesting TrajectorySegment data
*
*/
const MSG_JOB_CANCEL: &str = "cancel";

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
                if text == MSG_JOB_CANCEL {
                    println!("Received cancel signal for client {}", self.id);
                    if let Some(job) = self.job.take() {
                        job.write().unwrap().remove_client(&ctx.address());
                    }
                    println!("Done processing cancel for client {}", self.id);
                } else if let Ok(config) = serde_json::from_str::<PytfConfigMinimal>(&text) {
                    self.job_server.send(ClientReqJob {
                        config: config.into(),
                        client_id: self.id.clone(),
                        client_addr: ctx.address(),
                        client_prev_job: self.job.clone(),
                    })
                    .into_actor(self)
                    .then(|res, act, ctx| {
                        match res {
                            Ok(job) => {
                                    act.job = Some(job);
                                    act.job_server.do_send(AssignJobs {});
                                }
                            _ => ctx.stop(), // Something went wrong
                        }
                        fut::ready(())
                    })
                    .wait(ctx);
                } else if let Ok(segment_id) = text.parse::<usize>() {
                    // Client requesting data from frame with specified id
                    if let Some(job) = &self.job {
                        let job = job.read().unwrap();
                        let next_id = segment_id + 1;
                        if next_id < job.frames.len(){
                            if let Some(frame) = &job.frames[segment_id] {
                                ctx.binary(frame.data());
                            }
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



#[derive(Message)]
#[rtype(result="()")]
pub struct JobFailed {
    pub jobname: String,
}

impl Handler<JobFailed> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, msg: JobFailed, ctx: &mut Self::Context) -> Self::Result {
        if let Some(job) = &self.job {
            if job.read().unwrap().config.name == msg.jobname {
                ctx.text("failed");
            }
        }
    }
}
