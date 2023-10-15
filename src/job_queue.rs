use std::{sync::{Arc, RwLock, atomic::{AtomicBool, Ordering}}, collections::HashMap};
use actix::prelude::*;
use pytf_web::{
    pytf_config::PytfConfig,
    pytf_frame::TrajectorySegment
};

use crate::{
    client_session::{ClientWsSession, ClientForceDisconnect, TrajectoryPing},
    worker_session::{WorkerWsSession, WorkerPause, WorkerIdle}
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

// TODO: tell client when job failed?
// #[derive(Message)]
// #[rtype(result = "()")]
// pub struct JobFailed { jobname: String }

#[derive(Debug, Clone)]
struct WorkerHandle {
    addr: Addr<WorkerWsSession>,
    idle: Arc<AtomicBool>,
}
impl WorkerHandle {
    pub fn new(addr: Addr<WorkerWsSession>) -> Self {
        Self { addr, idle: Arc::new(AtomicBool::new(true)) }
    }
}


/// Server for connecting clients to workers and shuttling data
/// Currently runs on a single thread - maybe look into parallelising if it doesn't scale?
pub struct JobServer {
    /// Clients connected to this server
    client_sessions: HashMap<Arc<String>, ClientDetails>,

    /// Workers connected to this server
    worker_sessions: Vec<WorkerHandle>,

    /// Main job storage, indexed by job name (which is unique)
    job_lookup: HashMap<String, Job>,

    /// List of unfinished jobs - candidates for work requests
    unfinished_jobs: Vec<Job>,
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
            job_lookup,
            unfinished_jobs: Vec::with_capacity(64),
        }
    }

    fn count_idle_workers(&self) -> usize {
        self.worker_sessions
            .iter()
            .filter(|w| w.idle.load(Ordering::Acquire))
            .count()
    }

    fn send_job_to_worker(&self, job: JobAssignment, worker: &WorkerHandle, ctx: &mut <Self as Actor>::Context) {
        let idle = worker.idle.clone();
        worker.addr.send(job)
            .into_actor(self)
            .then(move | res, _act, _ctx| {
                match res {
                    Ok(true) => {
                        log::debug!("Sent new job to worker session");
                    }
                    Ok(false) => {
                        log::warn!("Worker failed to take job");
                        idle.store(true, Ordering::Release);
                    }
                    Err(e) => {
                        log::error!("Error while sending job assignment: {e}.");
                        idle.store(true, Ordering::Release); // There's a problem with the worker
                    }
                }
                fut::ready(())
            })
            .wait(ctx);
    }
}

impl Actor for JobServer {
    type Context = Context<Self>;
}

impl Handler<ClientConnect> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: ClientConnect, _ctx: &mut Self::Context) -> Self::Result {
        log::info!("Client {} connected", msg.id);

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
    }
}

impl Handler<WorkerIdle> for JobServer {
    type Result = ();

    /// Try to send freshly idle worker a new job, or mark it as idle if no jobs available
    fn handle(&mut self, msg: WorkerIdle, ctx: &mut Self::Context) -> Self::Result {
        for w in self.worker_sessions.iter() {
            if w.addr == msg.addr {
                if let Some(job) = self.unfinished_jobs.iter().filter(|job| is_job_runnable(job)).next() {
                    log::info!("Assigning new job to finished worker");
                    self.send_job_to_worker(JobAssignment { job: job.clone(), }, w, ctx);
                } else {
                    w.idle.store(true, Ordering::Release);
                }
                break
            }
        }
    }
}

fn is_job_runnable(job: &Job) -> bool {
    if let Ok(job) = job.try_read() {
        return (job.status == JobStatus::Waiting
        || matches!(job.status, JobStatus::Steal(_))
    ) && !job.clients.is_empty()
    }
    false
}

impl Handler<AssignJobs> for JobServer {
    type Result = ();

    fn handle(&mut self, _msg: AssignJobs, ctx: &mut Self::Context) -> Self::Result {
        log::debug!("Assigning unallocated jobs");
        let unassigned_jobs = self.unfinished_jobs.iter().filter(|job| is_job_runnable(job));

        let mut count = 0;
        for (job, worker) in unassigned_jobs.zip(
            self.worker_sessions.iter().filter(|w| w.idle.load(Ordering::Acquire))
        ) {
            worker.idle.store(false, Ordering::Release);
            self.send_job_to_worker(JobAssignment { job: job.clone(), }, worker, ctx);
            count += 1;
        }
        log::info!("Assigned {count} jobs");
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
            log::info!("Job with name {} already exists.", job_lock.config.name);
            log::debug!("Updated timestamp for job {}", job_lock.config.name);

            // If client wasn't already attached to that job, remove them from their old job
            if !job_lock.clients.contains(&msg.client_addr) {
                job_lock.clients.push(msg.client_addr.clone());
                log::debug!("Checking client's old job");
                if let Some(old_job) = msg.client_prev_job {
                    let mut old_job = old_job.write().unwrap();
                    log::debug!("Removing client {} from old job with name {}", msg.client_id, old_job.config.name);
                    old_job.remove_client(&msg.client_addr);
                }
                log::debug!("Finished cleaning up after {}", msg.client_id);
            } // NOTE: Assuming client_map can't get out of sync. Might need a test here if it can.
            log::debug!("Returning job handle");
            // Ping the client to let them know there are already frames available
            msg.client_addr.do_send(job_lock.build_ping());
            job.clone()
        } else {
            // Create new job and attach client
            log::info!("Creating new job for client {}", msg.client_id);
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
                log::info!("Removing client {} from old job with name {}", msg.client_id, old_job.config.name);
                old_job.remove_client(&msg.client_addr);
            }
            new_job
        }
    }
}

impl Handler<WorkerConnect> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: WorkerConnect, ctx: &mut Self::Context) -> Self::Result {
        log::info!("New worker connected");
        self.worker_sessions.push(WorkerHandle::new(msg.addr.clone()));
        if let Some(job) = self.unfinished_jobs.iter().filter(|job| is_job_runnable(job)).next() {
            log::info!("Assigning job to new worker");
            self.send_job_to_worker(
                JobAssignment { job: job.clone(), },
                self.worker_sessions.last().unwrap(),
                ctx
            );
        }
        log::debug!("Currently have {} workers, {} of which are idle.",
            self.worker_sessions.len(),
            self.count_idle_workers()
        );
    }
}

impl Handler<WorkerDisconnect> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: WorkerDisconnect, _ctx: &mut Self::Context) -> Self::Result {
        let Some(idx) = self.worker_sessions.iter().position(|w| w.addr == msg.addr) else {
            log::warn!("Disconnect message received for unknown worker");
            return
        };
        let _ = self.worker_sessions.swap_remove(idx);
        log::info!("Removed disconnected worker.");
        log::debug!("Currently have {} workers, of which {} are idle.",
            self.worker_sessions.len(),
            self.count_idle_workers()
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
                log::error!("Failed to store data for segment {} of job {}", msg.segment_id, msg.jobname);
            }
        } else {
            log::warn!("Received segment data for unknown job");
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

/// Data required to resume a job, packed into bytes
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PausedJobData {
    /// bytes containing first 10 lines of log file and contents of final-coordinates file
    pub data: actix_web::web::Bytes,
    // TODO: Could attach timestamp here to allow dropping/saving to disk of oldest `Steal` jobs
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StealingJob {
    pub data: PausedJobData,
    pub worker: Addr<WorkerWsSession>,
}

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
            log::warn!("Tried to remove client that wasn't attached");
            return
        };
        self.clients.swap_remove(client_idx);
        if self.clients.is_empty() {
            log::debug!("Job with name {} is now empty.", self.config.name);
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
            log::warn!("Received frame data for different job. Expected {jobname}, got {}", self.config.name);
            return AddSegmentResult::WrongJob(UnhandledTrajectorySegment { jobname: jobname.to_string(), segment_id, segment });
        }
        if segment_id > self.segments.len() {
            log::error!("Received segment ID of {segment_id} beyond end of expected segments ({})", self.segments.len());
            return AddSegmentResult::IdTooLarge;
        }
        // segment_id is 1-based
        if self.segments[segment_id - 1].replace(segment).is_some() {
            log::warn!("Received duplicate of segment {segment_id} for trajectory {}", self.config.name);
        } else {
            log::debug!("Stored segment {segment_id} of {} for job {}", self.segments.len(), self.config.name);
        }
        if segment_id > self.latest_segment {
            self.latest_segment = segment_id;
            log::debug!("Latest segment updated for job {}.", self.config.name);
        }
        AddSegmentResult::Ok
    }

    pub fn build_ping(&self) -> TrajectoryPing {
        TrajectoryPing {
            latest_segment: self.latest_segment,
            final_segment: self.latest_segment == self.segments.len(),
        }
    }

    /// Ping clients interested in job about new trajectory frame
    pub fn notify_clients(&self) {
        let ping = self.build_ping();
        log::debug!("Sending ping: {ping:?}");
        for client in &self.clients {
            client.do_send(ping);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddSegmentResult {
    /// Added successfully
    Ok,
    /// Job name didn't match
    WrongJob(UnhandledTrajectorySegment),
    /// Segment id was larger than the expected last segment
    IdTooLarge,
}

/// Add a segment to the specified job if the job's name matches the expected name, and notify
/// any attached clients that more frames are available.
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


