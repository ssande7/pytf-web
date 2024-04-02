use std::{time::{Duration, Instant}, sync::Arc};

use actix::prelude::*;
use actix_web_actors::ws;
use pytf_web::pytf_config::{PytfConfigMinimal, PytfConfig};

use crate::job_queue::{Job, JobServer, ClientConnect, ClientDisconnect, ClientReqJob, AssignJobs, JobInner, RegisterJob, AcceptedJob};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

/** MESSAGES TO CLIENT
*
* binary(b"{frame id: u32 little endian}{frame data}") => Frame of current job
*
* Others:
*/

/// text => There might be more frames available for current config.
/// Format is "{MSG_NEW_FRAMES}{{l:{latest_frame},f:{n_cycles}}}"
const MSG_NEW_FRAMES: &str = "new_frames";

/// text => Job has failed.
const MSG_JOB_FAILED: &str = "failed";

/// text => Requested segment ID (sent back) is unavailable
/// Format is "{MSG_SEG_UNAVAILABLE}{segment_id}"
const MSG_SEG_UNAVAILABLE: &str = "no_seg";

/// text => Job has been queued
const MSG_JOB_QUEUED: &str = "queued";



/** MESSAGES FROM CLIENT
*
* text("{PytfConfigMinimal as json}") => New configuration to run
*
* text("{segment_id, parseable to usize}") => Requesting TrajectorySegment data
*
* Others:
*/

/// text => Cancel the current job
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
                log::info!("Lost connection to client {}", act.id);
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

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        log::debug!("Sending disconnect signal for client {}", self.id);
        if !self.force_disconnect {
            if let Some(job) = &self.job {
                job.write().unwrap().remove_client(&ctx.address());
            }
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

    fn handle(&mut self, _msg: ClientForceDisconnect, ctx: &mut Self::Context) -> Self::Result {
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
                    log::info!("Received cancel signal for client {}", self.id);
                    if let Some(job) = self.job.take() {
                        job.write().unwrap().remove_client(&ctx.address());
                        ctx.text(MSG_JOB_CANCEL); // Confirm the cancel
                    } else {
                        log::warn!("Got a cancel request while not assigned a job");
                        ctx.text(MSG_JOB_QUEUED); // Queued works as a null response
                    }
                    log::debug!("Done processing cancel for client {}", self.id);
                } else if let Ok(config) = serde_json::from_str::<PytfConfigMinimal>(&text) {
                    log::info!("Received job config from client {}:\n{config:?}", self.id);
                    if let Some(old_job) = self.job.take() {
                        let mut old_job = old_job.write().unwrap();
                        log::info!("Removing client {} from old job with name {}", self.id, old_job.config.name);
                        old_job.remove_client(&ctx.address());
                    }
                    let config: PytfConfig = config.into();
                    self.job_server.send(ClientReqJob {
                        config: config.clone(),
                        client_id: self.id.clone(),
                        client_addr: ctx.address(),
                        client_prev_job: self.job.clone(),
                    })
                    .into_actor(self)
                    .then(|res, act, ctx| {
                        match res {
                            Ok(AcceptedJob::Existing(job)) => {
                                act.job = Some(job);
                                act.job_server.do_send(AssignJobs {});
                                ctx.text(MSG_JOB_QUEUED);
                            },
                            Ok(AcceptedJob::Finished(job)) => {
                                let ping = { job.read().unwrap().build_ping() };
                                act.job = Some(job.clone());
                                ctx.address().do_send(ping);
                            },
                            Ok(AcceptedJob::New) => {
                                // For new jobs, create on client thread
                                // since could involve slow read from disk
                                act.job_server.send(RegisterJob {
                                    job: JobInner::new(config),
                                    client: ctx.address(),
                                }).into_actor(act).then(|res, act, ctx| {
                                    if let Ok(job) = res {
                                        act.job = Some(job);
                                    } else {
                                        ctx.text(MSG_JOB_FAILED);
                                    }
                                    fut::ready(())
                                }).wait(ctx);
                                ctx.text(MSG_JOB_QUEUED);
                            }
                            Ok(AcceptedJob::Failed) => {
                                ctx.text(MSG_JOB_FAILED);
                            },
                            _ => ctx.stop(), // Something went wrong
                        }
                        fut::ready(())
                    })
                    .wait(ctx);
                } else if let Ok(segment_id) = text.parse::<usize>() {
                    log::debug!("Received request for segment {segment_id} from client {}: {text}", self.id);
                    // Client requesting data from frame with specified id
                    if let Some(job) = &self.job {
                        let job = job.read().unwrap();
                        if segment_id <= job.segments.len(){
                            if let Some(frame) = &job.segments[segment_id.saturating_sub(1)] {
                                log::debug!("Sending segment {segment_id} to client {}", self.id);
                                ctx.binary(frame.data());
                            } else {
                                log::debug!("Client requested segment {segment_id} which is not available.");
                                ctx.text(format!("{MSG_SEG_UNAVAILABLE}{}", segment_id));
                            }
                        } else {
                            log::debug!("Client requested segment {segment_id} which is beyond the end of the simulation.");
                            ctx.text(format!("{MSG_SEG_UNAVAILABLE}{}", segment_id));
                        }
                    } else {
                        log::debug!("Client requested segment {segment_id} but not assigned a job.");
                        ctx.text(format!("{MSG_SEG_UNAVAILABLE}{}", segment_id));
                    }
                } else {
                    log::warn!("Received unknown message from client {}", self.id);
                }
            }
            ws::Message::Binary(_) => log::warn!("Unexpected binary from client {}", self.id),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Message)]
#[rtype(result="()")]
pub struct TrajectoryPing {
    pub latest_segment: usize,
    pub final_segment: usize,
}

impl Handler<TrajectoryPing> for ClientWsSession {
    type Result = ();
    /// Notify client of possible extra trajectory data
    fn handle(&mut self, msg: TrajectoryPing, ctx: &mut Self::Context) -> Self::Result {
        if self.job.is_some() {
            ctx.text(format!("{}{{\"l\":{},\"f\":{}}}",
                MSG_NEW_FRAMES,
                msg.latest_segment,
                msg.final_segment
            ));
        }
    }
}



#[derive(Message)]
#[rtype(result="()")]
pub struct JobFailed {
    pub jobname: String,
}

impl Handler<JobFailed> for ClientWsSession {
    type Result = ();
    /// Notify client that job has failed
    fn handle(&mut self, msg: JobFailed, ctx: &mut Self::Context) -> Self::Result {
        // Client already removed from job's list at this point, so unlink pointer
        // to job as well.
        if let Some(job) = self.job.take() {
            if job.read().unwrap().config.name == msg.jobname {
                log::warn!("Sending fail message to client {} for job {}",
                    self.id, msg.jobname);
                ctx.text(MSG_JOB_FAILED);
            }
        }
    }
}
