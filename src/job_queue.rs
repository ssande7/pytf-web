use std::{sync::{Arc, RwLock}, collections::HashMap};
use actix::prelude::*;
use pytf_web::{
    pytf_config::PytfConfig,
    pytf_frame::TrajectorySegment
};

use crate::{
    client_session::{ClientWsSession, ClientForceDisconnect, TrajectoryPing},
    worker_session::{WorkerWsSession, WorkerPause}
};

// Client
#[derive(Message)]
#[rtype(result = "()")]
pub struct ClientConnect {
    pub id: Arc<String>,
    pub addr: Addr<ClientWsSession>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct ClientDisconnect {
    pub id: Arc<String>,
}

#[derive(Message)]
#[rtype(result = "Job")]
pub struct ClientReqJob {
    pub config: PytfConfig,
    pub client_id: Arc<String>,
    pub client_addr: Addr<ClientWsSession>,
    pub client_prev_job: Option<Job>,
}



// Worker
#[derive(Message)]
#[rtype(result = "()")]
pub struct WorkerConnect {
    pub addr: Addr<WorkerWsSession>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct WorkerDisconnect {
    pub addr: Addr<WorkerWsSession>,
}

pub struct ClientDetails {
    addr: Addr<ClientWsSession>,
    job: Option<Job>,
}



// Data

#[derive(Message)]
#[rtype(result = "()")]
pub struct AssignJobs {}

// #[derive(Message)]
// #[rtype(result = "()")]
// pub struct TrajectoryPacket {
//     jobname: Arc<String>,
//     pub bytes: Vec<u8>,
// }
//
//
// #[derive(Message)]
// #[rtype(result = "()")]
// pub struct JobFailed { jobname: String }


/// Server for connecting clients to workers and shuttling data
/// Currently runs on a single thread - maybe look into parallelising if it doesn't scale?
pub struct JobServer {
    /// Clients connected to this server
    client_sessions: HashMap<Arc<String>, ClientDetails>,

    /// Workers connected to this server
    worker_sessions: Vec<Addr<WorkerWsSession>>,

    idle_workers: Vec<Addr<WorkerWsSession>>,

    /// Main job storage, indexed by job name (which is unique)
    job_lookup: HashMap<String, Job>,

    /// List of unfinished jobs - candidates for work requests
    unfinished_jobs: Vec<Job>,

    /// Helper member for assigning jobs
    assignment_handler: AssignmentHandler,
}

impl JobServer {
    pub fn new() -> Self {
        let mut job_lookup = HashMap::with_capacity(128);
        let null_job = JobInner {
            config: PytfConfig::default(),
            status: JobStatus::Finished,
            clients: Vec::with_capacity(32),
            segments: Vec::new(),
            latest_segment: 0, // TODO: make this 1 and store empty system?
        };
        let null_name = null_job.config.name.clone();
        job_lookup.insert(null_name, null_job.wrap());
        Self {
            client_sessions: HashMap::with_capacity(64),
            worker_sessions: Vec::with_capacity(64),
            idle_workers: Vec::with_capacity(64),
            job_lookup,
            unfinished_jobs: Vec::with_capacity(64),
            assignment_handler: AssignmentHandler::default(),
        }
    }
}

impl Actor for JobServer {
    type Context = Context<Self>;
}

impl Handler<ClientConnect> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: ClientConnect, _ctx: &mut Self::Context) -> Self::Result {
        println!("Client {} connected", msg.id);

        if let Some(old_session) = self.client_sessions.insert(
            msg.id.clone(), ClientDetails { addr: msg.addr, job: None })
        {
            // Client started a new session before a previous one was closed
            // Remove interest from any previous job
            if let Some(old_job) = old_session.job {
                old_job.write().unwrap().remove_client(&old_session.addr);
            }
            // Tell the old session actor to end its connection
            old_session.addr.do_send(ClientForceDisconnect {});
        }
    }
}

impl Handler<ClientDisconnect> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: ClientDisconnect, _ctx: &mut Self::Context) -> Self::Result {
        self.client_sessions.remove(&msg.id);
        // TODO: cancel job if no clients left
    }
}

#[derive(Debug, Default, Copy, Clone)]
struct AssignmentHandler {
    retain: bool,
    skip: bool,
}

impl AssignmentHandler {
    #[inline(always)]
    fn reset(&mut self) {
        self.retain = true;
        self.skip = false;
    }

    /// Returns true if assignment loop should move on to next worker
    #[inline(always)]
    fn next_worker(&self) -> bool {
        !self.retain || self.skip
    }
}


impl Handler<AssignJobs> for JobServer {
    type Result = ();

    fn handle(&mut self, _msg: AssignJobs, ctx: &mut Self::Context) -> Self::Result {
        println!("Assigning any unallocated jobs");
        let mut unassigned_jobs = self.unfinished_jobs.iter().filter(|job| {
            if let Ok(job) = job.try_read() {
                return (job.status == JobStatus::Waiting
                    || matches!(job.status, JobStatus::Steal(_))
                ) && !job.clients.is_empty()
            }
            false
        });
        // let mut confirm_ctx = Context::new();
        println!("Currently have {} idle workers", self.idle_workers.len());
        let mut retain = vec![true; self.idle_workers.len()];
        for (retain, w) in retain.iter_mut().zip(self.idle_workers.iter()) {
            // Try sending jobs to an idle worker until it accepts one or we run out of jobs
            self.assignment_handler.reset();
            while let Some(job) = unassigned_jobs.next() {
                println!("Trying to assign job");
                self.assignment_handler.reset();
                w.send(JobAssignment { job: job.clone(), })
                    .into_actor(self)
                    .then(|res, act, _ctx| {
                        match res {
                            Ok(true) => {
                                println!("Sent new job to worker session");
                                act.assignment_handler.retain = false;
                            }
                            Ok(false) => { println!("Worker failed to take job"); }
                            Err(e) => {
                                eprintln!("Error while sending job assignment: {e}.");
                                act.assignment_handler.skip = true; // There's a problem with the worker, so skip over it
                            }
                        }
                        fut::ready(())
                    })
                    .wait(ctx);
                println!("Got confirmation");
                if self.assignment_handler.next_worker() { break }
            }
            *retain = self.assignment_handler.retain;
        }

        let mut ret = retain.iter();
        self.idle_workers.retain(|_| *ret.next().unwrap());
    }
}

impl Handler<ClientReqJob> for JobServer {
    type Result = Job;

    fn handle(&mut self, msg: ClientReqJob, _ctx: &mut Self::Context) -> Self::Result {
        // Check whether job already exists.
        // Keep job_lookup locked while we work with it to avoid races
        // (i.e. we can only add one new job at a time)
        let jobname = msg.config.name.clone();
        let existing = self.job_lookup.get(&jobname).and_then(|j| Some(j.clone()));
        if let Some(job) = existing {
            // Attach client to job.
            let mut job_lock = job.write().unwrap();
            println!("Job with name {} already exists.", job_lock.config.name);
            println!("Updated timestamp for job {}", job_lock.config.name);

            // If client wasn't already attached to that job, remove them from their old job
            if !job_lock.clients.contains(&msg.client_addr) {
                job_lock.clients.push(msg.client_addr.clone());
                println!("Checking client's old job");
                if let Some(old_job) = msg.client_prev_job {
                    let mut old_job = old_job.write().unwrap();
                    println!("Removing client {} from old job with name {}", msg.client_id, old_job.config.name);
                    old_job.remove_client(&msg.client_addr);
                }
                println!("Finished cleaning up after {}", msg.client_id);
            } // NOTE: Assuming client_map can't get out of sync. Might need a test here if it can.
            println!("Returning job handle");
            // Ping the client to let them know there are already frames available
            msg.client_addr.do_send(TrajectoryPing {
                latest_segment: job_lock.latest_segment,
            });
            job.clone()
        } else {
            // Create new job and attach client
            println!("Creating new job for client {}", msg.client_id);
            let mut clients = Vec::with_capacity(32);
            clients.push(msg.client_addr.clone());
            // Each frame pack is one cycle worth of data
            let new_job = JobInner {
                segments: vec![None; msg.config.n_cycles],
                latest_segment: 0,
                config: msg.config,
                status: JobStatus::Waiting,
                clients,
            }.wrap();
            self.job_lookup.insert(jobname, new_job.clone());
            // Add new job to list of unfinished ones
            self.unfinished_jobs.push(new_job.clone());
            if let Some(old_job) = msg.client_prev_job {
                let mut old_job = old_job.write().unwrap();
                println!("Removing client {} from old job with name {}", msg.client_id, old_job.config.name);
                old_job.remove_client(&msg.client_addr);
            }
            new_job
        }
    }
}

impl Handler<WorkerConnect> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: WorkerConnect, ctx: &mut Self::Context) -> Self::Result {
        println!("New worker connected");
        self.worker_sessions.push(msg.addr.clone());
        self.idle_workers.push(msg.addr);
        println!("Currently have {} workers, {} of which are idle.",
            self.worker_sessions.len(),
            self.idle_workers.len()
        );
        ctx.address().do_send(AssignJobs {});
    }
}

impl Handler<WorkerDisconnect> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: WorkerDisconnect, _ctx: &mut Self::Context) -> Self::Result {
        let Some(idx) = self.worker_sessions.iter().position(|w| *w == msg.addr) else {
            println!("Disconnect message received for unknown worker");
            return
        };
        let _ = self.worker_sessions.swap_remove(idx);
        if let Some(idx) = self.idle_workers.iter().position(|w| *w == msg.addr) {
            let _ = self.idle_workers.swap_remove(idx);
        };
        println!("Removed disconnected worker.\n\
            Currently have {} workers, of which {} are idle.",
            self.worker_sessions.len(),
            self.idle_workers.len()
        );
    }
}

#[derive(Debug, Message, PartialEq, Eq)]
#[rtype(result="()")]
pub struct UnhandledTrajectorySegment {
    pub jobname: String,
    pub segment_id: usize,
    pub segment: TrajectorySegment,
}

impl Handler<UnhandledTrajectorySegment> for JobServer {
    type Result = ();
    fn handle(&mut self, msg: UnhandledTrajectorySegment, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(job) = self.job_lookup.get(&msg.jobname) {
            if AddSegmentResult::Ok != job_add_seg_and_notify(job, &msg.jobname, msg.segment_id, msg.segment) {
                eprintln!("Failed to store data for segment {} of job {}", msg.segment_id, msg.jobname);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct JobInner {
    pub config: PytfConfig,
    pub status: JobStatus,
    pub clients: Vec<Addr<ClientWsSession>>,
    pub segments: Vec<Option<TrajectorySegment>>,
    pub latest_segment: usize,
}
pub type Job = Arc<RwLock<JobInner>>;


/// TODO: Stealing requires input-coordinates, final-coordinates and log file of latest run.
///       Worker should package these as .tar when pausing a job and send it to the main
///       server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PausedJobData {
    /// bytes of a .tar file containing the latest input-coordinates, final-coordinates and log
    /// files of the paused job to allow resuming on a different worker.
    pub data: actix_web::web::Bytes,
    // TODO: Could attach timestamp here to allow dropping/saving to disk of oldest `Steal` jobs
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StealingJob {
    pub data: PausedJobData,
    pub worker: Addr<WorkerWsSession>,
}

// TODO: Make worker an Addr to the WorkerWsSession
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobStatus {
    /// Waiting to be run
    Waiting,
    /// Running on the specified worker
    Running(Addr<WorkerWsSession>),
    /// Paused, last worked on by specified worker
    Paused(Addr<WorkerWsSession>),
    /// Ready to steal
    Steal(PausedJobData),
    /// Currently being stolen by specified worker
    Stealing(PausedJobData, Addr<WorkerWsSession>),
    /// Job has completed
    Finished,
    /// Job has failed
    Failed,
}

impl JobInner {
    pub fn wrap(self) -> Job {
        Arc::new(RwLock::new(self))
    }

    pub fn remove_client(&mut self, client: &Addr<ClientWsSession>) {
        let Some(client_idx) = self.clients.iter().position(|c| c == client) else {
            println!("Tried to remove client that wasn't attached");
            return
        };
        self.clients.swap_remove(client_idx);
        if self.clients.is_empty() {
            println!("Job with name {} is now empty.", self.config.name);
            if let JobStatus::Running(worker) = &self.status {
                worker.do_send(WorkerPause { jobname: self.config.name.clone() });
                self.status = JobStatus::Paused(worker.clone());
            }
        }
    }

    /// Add a segment for `segment_id` if `expected_name` matches this job's name
    pub fn add_segment(&mut self, expected_name: impl AsRef<str>, segment_id: usize, segment: TrajectorySegment)
    -> AddSegmentResult {
        let jobname = expected_name.as_ref();
        if self.config.name != jobname {
            println!("Received frame data for different job. Expected {jobname}, got {}", self.config.name);
            // TODO: Find that job? This could happen when one job is replaced
            // with another, but finishes its current segment before exiting.
            return AddSegmentResult::WrongJob(UnhandledTrajectorySegment { jobname: jobname.to_string(), segment_id, segment });
        }
        if segment_id > self.segments.len() {
            println!("Received segment ID of {segment_id} beyond end of expected segments ({})", self.segments.len());
            return AddSegmentResult::IdTooLarge;
        }
        // segment_id is 1-based
        if self.segments[segment_id - 1].replace(segment).is_some() {
            println!("WARNING: Received duplicate of segment {segment_id} for trajectory {}", self.config.name);
        } else {
            println!("Stored segment {segment_id} for job {}", self.config.name);
        }
        if segment_id > self.latest_segment {
            self.latest_segment = segment_id;
            println!("Latest segment updated for job {}.", self.config.name);
        }
        AddSegmentResult::Ok
    }

    /// Ping clients interested in job about new trajectory frame
    pub fn notify_clients(&self) {
        for client in &self.clients {
            client.do_send(TrajectoryPing {
                latest_segment: self.latest_segment,
            });
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddSegmentResult {
    Ok,
    WrongJob(UnhandledTrajectorySegment),
    IdTooLarge,
}

pub fn job_add_seg_and_notify(job: &Job, expected_name: impl AsRef<str>, segment_id: usize, segment: TrajectorySegment)
-> AddSegmentResult {
    let out = {
        let mut job = job.write().unwrap();
        job.add_segment(expected_name, segment_id, segment)
    };
    if out == AddSegmentResult::Ok {
        let job = job.read().unwrap();
        job.notify_clients();
    }
    out
}

/// Job assignment message. Worker returns `true` if job assigned successfully, `false` otherwise.
#[derive(Debug, Clone, Message)]
#[rtype(result="bool")]
pub struct JobAssignment {
    /// Config to run
    pub job: Job,
}


