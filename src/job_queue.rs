use std::{
    sync::{Arc, RwLock, atomic::{AtomicBool, Ordering}, OnceLock},
    collections::HashMap,
    time::{Duration, Instant},
    io::{BufReader, BufWriter, Read, Write}, path::PathBuf, fmt::Display
};
use actix::prelude::*;
use actix_web::web::Bytes;
use pytf_web::{
    pytf_config::PytfConfig,
    pytf_frame::TrajectorySegment
};

use crate::{
    client_session::{ClientWsSession, ClientForceDisconnect, TrajectoryPing},
    worker_session::{WorkerWsSession, WorkerPause, WorkerIdle}
};

/// How frequently to check for jobs to archive.
/// Doubles as age at which jobs are eligible for archiving.
/// TODO: make this configurable
const JOB_CLEANUP_INTERVAL: Duration = Duration::from_secs(150);
const MAX_JOB_AGE: Duration = Duration::from_secs(300);

/// Directory to store archived jobs.
pub static ARCHIVE_DIR: OnceLock<PathBuf> = OnceLock::new();

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
#[rtype(result = "AcceptedJob")]
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
        if let Err(e) = std::fs::create_dir_all(ARCHIVE_DIR.get().unwrap()) {
            log::warn!("Failed to create archive directory with error \"{e}\". Old jobs will not be archived!");
        }
        let mut job_lookup = HashMap::with_capacity(128);
        let null_job = JobInner::new(PytfConfig::default());
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
        let job_handle = job.job.clone();
        worker.addr.send(job)
            .into_actor(self)
            .then(move | res, _act, _ctx| {
                match res {
                    Ok(true) => {
                        log::debug!("Sent new job to worker session");
                        let job = job_handle.read().unwrap();
                        job.notify_clients_no_timestamp();
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

    fn start_cleanup_timer(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(JOB_CLEANUP_INTERVAL, |act, _ctx| {
            act.cleanup_jobs(Instant::now());
        });
    }

    fn cleanup_jobs(&mut self, now: Instant) {
        self.unfinished_jobs.retain(|job| {
            if let Ok(mut job_lock) = job.try_write() {
                let retain = job_lock.archive_if_ready(&now);
                if !retain {
                    self.job_lookup.remove(&job_lock.config.name);
                }
                retain
            } else { true }
        });
        self.job_lookup.retain(|_jobname, job| {
            if let Ok(mut job_lock) = job.try_write() {
                job_lock.archive_if_ready(&now)
            } else { true }
        })
    }

    fn assign_jobs(&mut self, ctx: &mut <Self as Actor>::Context) {
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

impl Actor for JobServer {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        log::debug!("Starting cleanup timer");
        self.start_cleanup_timer(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        log::debug!("Triggering final cleanup.");
        self.cleanup_jobs(Instant::now() + 2*MAX_JOB_AGE);
    }
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
        self.assign_jobs(ctx);
    }
}

pub enum AcceptedJob {
    /// Creating a new job
    New,
    /// Attaching to an existing job
    Existing(Job),
    /// Attaching to a finished job
    Finished(Job),
    /// Job exists, but has failed
    Failed,
}

impl Handler<ClientReqJob> for JobServer {
    type Result = MessageResult<ClientReqJob>;

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

            // If client wasn't already attached to that job, remove them from their old job
            if !job_lock.clients.contains(&msg.client_addr) {
                job_lock.clients.push(msg.client_addr.clone());
            }
            match job_lock.status {
                JobStatus::Finished => {
                    drop(job_lock);
                    MessageResult(AcceptedJob::Finished(job))
                },
                JobStatus::Failed => MessageResult(AcceptedJob::Failed),
                _ => {
                    drop(job_lock);
                    MessageResult(AcceptedJob::Existing(job))
                },
            }
        } else {
            MessageResult(AcceptedJob::New)
        }
    }
}

#[derive(Message)]
#[rtype(result="Job")]
pub struct RegisterJob {
    pub client: Addr<ClientWsSession>,
    pub job: JobInner,
}
impl Handler<RegisterJob> for JobServer {
    type Result = Job;
    fn handle(&mut self, msg: RegisterJob, ctx: &mut Self::Context) -> Self::Result {
        let jobname = msg.job.config.name.clone();
        if let Some(job) = self.job_lookup.get(&jobname) {
            // Job may have been registered by another client while loading
            job.write().unwrap().clients.push(msg.client);
            job.clone()
        } else {
            let mut job = msg.job;
            let finished = job.status == JobStatus::Finished;
            job.clients.push(msg.client);
            if finished { job.notify_clients(); }
            let job = job.wrap();
            self.job_lookup.insert(jobname, job.clone());
            if !finished { self.unfinished_jobs.push(job.clone()); }
            self.assign_jobs(ctx);
            job
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
            match job_add_seg_and_notify(job, &msg.jobname, msg.segment_id, msg.segment) {
                AddSegmentResult::IdTooLarge | AddSegmentResult::WrongJob(_)
                    => log::error!("Failed to store data for segment {} of job {}", msg.segment_id, msg.jobname),
                _ => (),
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
    pub timestamp: Instant,
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
    /// Job is archived to disk. Could be Waiting, Steal, Finished or Failed
    Archived,
}

impl Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Waiting       => "Waiting",
            Self::Running(_)    => "Running",
            Self::Paused(_)     => "Paused",
            Self::Steal(_)      => "Ready to Steal",
            Self::Stealing(_,_) => "Being Stolen",
            Self::Finished      => "Finished",
            Self::Failed        => "Failed",
            Self::Archived      => "Archived",
        })
    }
}

impl JobInner {
    /// Create a new job. Will attempt to load from archive on disk,
    /// or create a new job with the Waiting status if loading fails
    pub fn new(config: PytfConfig) -> Self {
        if ARCHIVE_DIR.get().unwrap().join(config.archive_name()).is_file() {
             match Self::load(config.clone()) {
                Ok(job) => {
                    if job.status == JobStatus::Finished {
                        job.notify_clients_no_timestamp();
                    }
                    // For "Steal" jobs, the job will just be queued and
                    // report back segments once it starts running.
                    return job;
                },
                Err(e) => {
                    log::warn!("Error \"{e}\" while loading archived job \"{}\". Generating new job instead.", config.name);
                }
            }
        }
        JobInner {
            segments: vec![None; config.n_cycles],
            latest_segment: 0,
            config,
            status: JobStatus::Waiting,
            clients: Vec::with_capacity(32),
            timestamp: Instant::now(),
        }
    }

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
            let status = std::mem::replace(&mut self.status, JobStatus::Waiting); // Store cheap temp. state
            self.status = match status {
                JobStatus::Running(worker) => {
                    worker.do_send(WorkerPause { jobname: self.config.name.clone() });
                    JobStatus::Paused(worker)
                },
                JobStatus::Stealing(pause_data, worker) => {
                    worker.do_send(WorkerPause { jobname: self.config.name.clone() });
                    JobStatus::Steal(pause_data)
                }
                other => other,
            };
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
        self.timestamp = Instant::now();
        if self.clients.is_empty() { AddSegmentResult::NoClients }
        else { AddSegmentResult::Ok }
    }

    pub fn build_ping(&self) -> TrajectoryPing {
        TrajectoryPing {
            latest_segment: self.latest_segment,
            final_segment: self.segments.len(),
        }
    }

    /// Ping clients interested in job about new trajectory frame
    /// self needs to be mutable to save timestamp
    pub fn notify_clients(&mut self) {
        self.timestamp = Instant::now();
        self.notify_clients_no_timestamp();
    }

    pub fn notify_clients_no_timestamp(&self) {
        let ping = self.build_ping();
        log::debug!("Sending ping: {ping:?}");
        for client in &self.clients {
            client.do_send(ping);
        }
    }

    pub fn archive_if_ready(&mut self, now: &Instant) -> bool {
        // If job was recently touched or has attached clients, don't archive it
        if now.duration_since(self.timestamp) < MAX_JOB_AGE || self.clients.len() > 0 {
            return true;
        }
        match self.status {
            JobStatus::Finished | JobStatus::Steal(_) => {
                // Job is stale and has no attached clients, so archive it to disk
                // and remove it from the job lookup table.
                match self.archive() {
                    Ok(_) => false,
                    Err(e) => {
                        log::warn!("Failed to archive job \"{}\" with error \"{e}\"", self.config.name);
                        self.timestamp = now.clone(); // Avoid retrying immediately
                        true
                    }
                }
            },
            JobStatus::Waiting | JobStatus::Failed => false, // Just remove abandoned Waiting jobs.
            _ => true,
        }
    }

    pub fn archive(&mut self) -> std::io::Result<()> {
        let fid = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(ARCHIVE_DIR.get().unwrap().join(self.config.archive_name()))?;
        let mut fid = BufWriter::new(fid);
        // No need to write config, since we only need to open the file again if we get another
        // (matching) config that leads to it.
        if let JobStatus::Steal(pause_data) = &self.status {
            fid.write_all(&pause_data.data.len().to_le_bytes())?;
            fid.write_all(&pause_data.data)?;
        } else {
            fid.write_all(&(0 as usize).to_le_bytes())?;
            let status: usize = match self.status {
                JobStatus::Finished => 1,
                // Space to include other job statuses if needed
                _ => {
                    log::error!("Invalid job status while writing archived job");
                    return Err(std::io::ErrorKind::InvalidData.into());
                }
            };
            fid.write_all(&status.to_le_bytes())?;
        }
        fid.write_all(&self.latest_segment.to_le_bytes())?;
        for seg in self.segments.iter().take(self.latest_segment) {
            if let Some(seg) = seg {
                fid.write_all(&seg.data().len().to_le_bytes())?;
                fid.write_all(&seg.data())?;
            } else {
                fid.write_all(&(0 as usize).to_le_bytes())?;
            }
        }
        fid.flush()?;
        log::debug!("Archived job {}", self.config.name);
        self.status = JobStatus::Archived;
        Ok(())
    }

    pub fn load(config: PytfConfig) -> std::io::Result<Self> {
        let fid = std::fs::OpenOptions::new()
            .read(true)
            .open(ARCHIVE_DIR.get().unwrap().join(config.archive_name()))?;
        let mut fid = BufReader::new(fid);

        let pause_bytes = read_le_usize(&mut fid)?;
        let status = if pause_bytes > 0 {
            let mut resume_data: Vec<u8> = vec![0u8; pause_bytes];
            fid.read_exact(&mut resume_data)?;
            JobStatus::Steal(PausedJobData { data: Bytes::from(resume_data) })
        } else {
            match read_le_usize(&mut fid)? {
                1 => JobStatus::Finished,
                _ => {
                    log::error!("Invalid job status while reading archived job");
                    return Err(std::io::ErrorKind::InvalidData.into());
                }
            }
        };
        let latest_segment = read_le_usize(&mut fid)?;
        let mut segments: Vec<Option<TrajectorySegment>> = vec![None; config.n_cycles];
        for i in 0..latest_segment {
            let bytes = read_le_usize(&mut fid)?;
            if bytes > 0 {
                let mut seg_data: Vec<u8> = vec![0u8; bytes];
                fid.read_exact(&mut seg_data)?;
                segments[i] = Some(TrajectorySegment { data: Bytes::from(seg_data) });
            }
        }
        log::debug!("Loaded job {} from archive", config.name);
        Ok(Self {
            config,
            status,
            clients: Vec::new(), // Not setting capacity since this could be overwritten
            segments,
            latest_segment,
            timestamp: Instant::now(),
        })
    }
}

fn read_le_usize(fid: &mut BufReader<impl Read>) -> std::io::Result<usize> {
    let mut out = [0u8; 8];
    fid.read_exact(&mut out)?;
    Ok(usize::from_le_bytes(out))
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddSegmentResult {
    /// Added successfully
    Ok,
    /// Job name didn't match
    WrongJob(UnhandledTrajectorySegment),
    /// Segment id was larger than the expected last segment
    IdTooLarge,
    /// No clients left attached to the job
    NoClients,
}

/// Add a segment to the specified job if the job's name matches the expected name, and notify
/// any attached clients that more frames are available.
pub fn job_add_seg_and_notify(job: &Job, expected_name: impl AsRef<str>, segment_id: usize, segment: TrajectorySegment)
-> AddSegmentResult {
    let mut job = job.write().unwrap();
    let out = job.add_segment(expected_name, segment_id, segment);
    if out == AddSegmentResult::Ok {
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


