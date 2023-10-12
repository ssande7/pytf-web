use std::{time::{Duration, Instant}, str};

use actix::prelude::*;
use actix_web_actors::ws;
use pytf_web::{
    pytf_frame::TrajectorySegment,
    worker_client::{
        DONE_HEADER,
        FAILED_HEADER,
        JOB_HEADER,
        PAUSE_HEADER,
        STEAL_HEADER,
        SEGMENT_HEADER, RESUME_HEADER,
    }, split_nullterm_utf8_str
};

use crate::{
    job_queue::{
        Job, JobServer, AssignJobs,
        WorkerConnect, WorkerDisconnect,
        JobAssignment, JobStatus,
        PausedJobData, UnhandledTrajectorySegment, AddSegmentResult, job_add_seg_and_notify
    },
    client_session::JobFailed,
};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

const WORKER_TIMEOUT: Duration = Duration::from_secs(90);

/** MESSAGES TO WORKER NODE:
*
* binary(b"job\0{config}")=> new job to run
*
* binary(b"pause\0{jobname}") => Stop running the job with jobname and send
*                               the data from the latest complete run.
*
* binary(b"steal\0{config}\0{prev_data}") => existing job to continue
*
*/

/** MESSAGES FROM WORKER NODE:
*
* binary(b"done\0{jobname}") => Job is finished
*
* binary(b"fail\0{jobname}") => Job has failed
*
* binary(b"seg\0{jobname}\0{segment_data}") => segment of trajectory
*
* binary(b"resume\n{jobname}") => Successfully resumed job
*
*/



/// Handler for a connected worker node
#[derive(Debug)]
pub struct WorkerWsSession {
    pub heartbeat: Instant,

    pub job: Option<Job>,

    pub job_server: Addr<JobServer>,
}

#[derive(Debug, Clone, Message, serde::Serialize, serde::Deserialize)]
#[rtype(result="()")]
pub struct WorkerPause {
    pub jobname: String,
}

#[derive(Debug, Clone, Message)]
#[rtype(result="()")]
pub struct WorkerIdle {
    pub addr: Addr<WorkerWsSession>,
}

impl WorkerWsSession {
    pub fn new(job_server: Addr<JobServer>) -> Self {
        Self {
            heartbeat: Instant::now(),
            job: None,
            job_server,
        }
    }

    fn heartbeat(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.heartbeat) > WORKER_TIMEOUT {
                log::info!("Lost connection to worker");
                // act.job_server.do_send(WorkerDisconnect { id: act.id });
                ctx.stop();
                return;
            }
            ctx.ping(b"");
        });
    }
}


impl Actor for WorkerWsSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.heartbeat(ctx);

        let addr = ctx.address();
        self.job_server
            .send(WorkerConnect { addr })
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
        self.job_server.do_send(WorkerDisconnect { addr: ctx.address() });
        if let Some(job) = &self.job {
            let mut job = job.write().unwrap();
            match &job.status {
                JobStatus::Running(addr)
                    | JobStatus::Paused(addr)
                    if *addr == ctx.address()
                    => {
                    // Job still in progress on my worker and no pause data, so invalidate it
                    job.status = JobStatus::Waiting;
                    // Notify server that new job may be available
                    self.job_server.do_send(AssignJobs {})
                },
                JobStatus::Stealing(data, addr) if *addr == ctx.address() => {
                    // Job was in the process of being stolen, so go back to waiting for worker
                    job.status = JobStatus::Steal(data.clone());
                    // Notify server that new job may be available
                    self.job_server.do_send(AssignJobs {})
                }
                _ => (),

            }
        }
        Running::Stop
    }
}

impl Handler<JobAssignment> for WorkerWsSession {
    type Result = bool;

    /// Forward on job assignment details to the worker node
    fn handle(&mut self, msg: JobAssignment, ctx: &mut Self::Context) -> Self::Result {
        let job = msg.job;
        let mut job_lock = job.write().unwrap();
        log::info!("Got job assignment: {job_lock:?}");
        match &job_lock.status {
            JobStatus::Waiting => {
                job_lock.status = JobStatus::Running(ctx.address());
                match serde_json::to_string(&job_lock.config) {
                    Ok(config) => {
                        // Sanitize old job in case messages got jumbled (probably not needed)
                        if let Some(old_job) = self.job.replace(job.clone()) {
                            let mut old_job = old_job.write().unwrap();
                            match &old_job.status {
                                JobStatus::Running(addr) if *addr == ctx.address() => {
                                    log::warn!("Assigned new job, but already had a running job. \
                                        Marking as Paused and assuming pause data will come.");
                                    old_job.status = JobStatus::Paused(addr.clone());
                                }
                                JobStatus::Stealing(data, addr) if *addr == ctx.address() => {
                                    log::warn!("Assigned new job, but already stealing a job. \
                                        Marking as Steal to be picked up by another worker.");
                                    old_job.status = JobStatus::Steal(data.clone());
                                    self.job_server.do_send(AssignJobs {});
                                }
                                _ => (),
                            };
                        }
                        ctx.binary([JOB_HEADER, config.as_bytes()].concat());
                        log::info!("Sent job to worker");
                        true
                    }
                    Err(e) => {
                        job_lock.status = JobStatus::Waiting;
                        log::error!("Something went wrong serializing job assignment {job_lock:?}: {e}");
                        false
                    }
                }
            }
            JobStatus::Steal(data) => {
                let data = data.clone();
                job_lock.status = JobStatus::Stealing(data.clone(), ctx.address());
                match serde_json::to_string(&job_lock.config) {
                    Ok(config) => {
                        // Sanitize old job in case messages got jumbled (probably not needed)
                        if let Some(old_job) = self.job.replace(job.clone()) {
                            let mut old_job = old_job.write().unwrap();
                            match &old_job.status {
                                JobStatus::Running(addr) if *addr == ctx.address() => {
                                    log::warn!("Assigned job to steal, but already had a running job. \
                                        Marking as Paused and assuming pause data will come.");
                                    old_job.status = JobStatus::Paused(addr.clone());
                                }
                                JobStatus::Stealing(data, addr) if *addr == ctx.address() => {
                                    log::warn!("Assigned job to steal, but already stealing a job. \
                                        Marking as Steal to be picked up by another worker.");
                                    old_job.status = JobStatus::Steal(data.clone());
                                    self.job_server.do_send(AssignJobs {});
                                }
                                _ => (),
                            };
                        }
                        ctx.binary([STEAL_HEADER, config.as_bytes(), b"\0", data.data.as_ref()].concat());
                        log::info!("Sent resume job to worker");
                        true
                    }
                    Err(e) => {
                        job_lock.status = JobStatus::Steal(data);
                        log::error!("Something went wrong serializing job assignment {job_lock:?}: {e}");
                        false
                    }
                }
            }
            _ => false,
        }
    }
}

impl Handler<WorkerPause> for WorkerWsSession {
    type Result = ();

    fn handle(&mut self, msg: WorkerPause, ctx: &mut Self::Context) -> Self::Result {
        ctx.binary([PAUSE_HEADER, msg.jobname.as_bytes()].concat());
    }
}

// Incoming stream from worker
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WorkerWsSession {
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
            ws::Message::Text(_) => {
                log::warn!("Unexpected text from worker");
            }
            ws::Message::Binary(mut bytes) => {
                if bytes.starts_with(PAUSE_HEADER) {
                    log::info!("Worker session received pause data");
                    // Format is b"pause\0{jobname}\0{pause_data}"
                    let _ = bytes.split_to(PAUSE_HEADER.len());
                    let jobname = match split_nullterm_utf8_str(&mut bytes) {
                        Ok(jobname) => jobname,
                        Err(e) => {
                            log::error!("Error reading jobname from pause data: {e}");
                            return;
                        }
                    };
                    if let Some(job) = &self.job {
                        {
                            let job = job.read().unwrap();
                            if job.config.name != jobname {
                                log::error!("Received pause data for job with different name. Expected {jobname}, got {}", job.config.name);
                                return
                            }
                        }
                        {
                            let mut job = job.write().unwrap();
                            match &job.status {
                                JobStatus::Paused(addr) if *addr == ctx.address() => {
                                    job.status = JobStatus::Steal(PausedJobData {
                                        data: bytes,
                                    });
                                }
                                _ => {
                                    log::error!(
                                        "Job {} in unexpected state when trying to set up for stealing: {:?}",
                                        jobname, job.status);
                                }
                            }
                        }
                        self.job = None;
                    } else {
                        log::error!("Got pause data, but don't have a job!");
                    }
                    // Finished with old job, so can start a new one
                    self.job_server.do_send(WorkerIdle {addr: ctx.address()});

                } else if bytes.starts_with(SEGMENT_HEADER) {
                    log::info!("Worker session received segment data");
                    // Format is b"seg\0{jobname}\0{segment_id: u32 little endian}{rest_of_frame_data}"
                    let _ = bytes.split_to(SEGMENT_HEADER.len());
                    let jobname = match split_nullterm_utf8_str(&mut bytes) {
                        Ok(jobname) => jobname,
                        Err(e) => {
                            log::error!("Error reading jobname from segment data: {e}");
                            return;
                        }
                    };
                    // 4 bytes for segment id
                    if bytes.len() < 4 {
                        log::error!("Malformed binary segment from worker");
                        return
                    }
                    // Package back core data with header removed to be forwarded on to clients
                    let segment_id = u32::from_le_bytes(bytes[..4].as_ref().try_into().unwrap()) as usize;
                    let segment = TrajectorySegment::new(bytes);
                    if let Some(job) = &self.job {
                        if let AddSegmentResult::WrongJob(seg) = job_add_seg_and_notify(job, jobname, segment_id, segment) {
                            log::warn!("Received segment for different job. Forwarding on for processing.");
                            self.job_server.do_send(seg);
                        }
                    } else {
                        log::warn!("Received segment, but not assigned a job. Forwarding on for processing.");
                        self.job_server.do_send(UnhandledTrajectorySegment{ jobname, segment_id, segment });
                    }
                } else if bytes.starts_with(FAILED_HEADER) {
                    log::warn!("Worker session received fail message");
                    let _ = bytes.split_to(FAILED_HEADER.len());
                    let jobname = match str::from_utf8(&bytes) {
                        Ok(jobname) => jobname,
                        Err(e) => {
                            log::error!("Error reading failed jobname: {e}");
                            return
                        }
                    };
                    if self.job.as_ref().and_then(
                        |j| Some(j.read().unwrap().config.name == jobname)
                    ) != Some(true) {
                        log::warn!("Received failed signal for a different job");
                        return;
                    }
                    if let Some(job) = self.job.take() {
                        let clients = {
                            let mut job_lock = job.write().unwrap();
                            job_lock.status = JobStatus::Failed;
                            let clients = std::mem::replace(&mut job_lock.clients, Vec::new());
                            clients
                        };
                        for client in clients {
                            client.do_send(JobFailed { jobname: jobname.to_owned(), });
                        }
                    }
                    // Get a new job, or add worker back to idle list
                    self.job_server.do_send(WorkerIdle {addr: ctx.address()});
                } else if bytes.starts_with(DONE_HEADER) {
                    let _ = bytes.split_to(DONE_HEADER.len());
                    let jobname = match str::from_utf8(&bytes) {
                        Ok(jobname) => jobname,
                        Err(e) => {
                            log::error!("Error reading finished jobname: {e}");
                            return
                        }
                    };
                    log::info!("Job {jobname} is finished.");
                    // Mark job as done and add worker to idle list
                    if let Some(job) = self.job.take() {
                        let mut job_lock = job.write().unwrap();
                        if job_lock.config.name == jobname {
                            job_lock.status = JobStatus::Finished;
                        } else {
                            self.job = Some(job.clone());
                            log::error!("Received done message for different job. This should never happen.");
                        }
                    } else {
                        log::error!("Received done message, but don't have a job. This should never happen.");
                    }
                    self.job_server.do_send(WorkerIdle {addr: ctx.address()});
                } else if bytes.starts_with(RESUME_HEADER) {
                    let _ = bytes.split_to(RESUME_HEADER.len());
                    let jobname = match str::from_utf8(&bytes) {
                        Ok(jobname) => jobname,
                        Err(e) => {
                            log::error!("Error reading failed jobname: {e}");
                            return
                        }
                    };
                    if let Some(job) = &self.job {
                        let mut job = job.write().unwrap();
                        if job.config.name == jobname {
                            if let JobStatus::Stealing(..) = job.status {
                                job.status = JobStatus::Running(ctx.address());
                            }
                        } else {
                            log::error!("Received resume message for different job!");
                        }
                    }
                } else {
                    log::error!("Received unknown message!");
                }
            }
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
