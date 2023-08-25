use std::{sync::{Arc, Mutex, RwLock, atomic::{AtomicUsize, Ordering}}, collections::HashMap, time::Duration};
use actix::{clock::Instant, Addr, Actor, Message, Handler, Context};
use actix_web_actors::ws;
use awc::Client;
use pytf_web::pytf_config::PytfConfig;

use crate::{client_session::{ClientWsSession, ClientForceDisconnect}, worker_session::WorkerWsSession};


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
#[rtype(result = "usize")]
pub struct WorkerConnect {
    pub addr: Addr<WorkerWsSession>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct WorkerDisconnect {
    pub id: usize,
}

pub struct ClientDetails {
    addr: Addr<ClientWsSession>,
    job: Option<Job>,
}



// Data

#[derive(Message)]
#[rtype(result = "()")]
pub struct TrajectoryPacket {
    jobname: Arc<String>,
    pub bytes: Vec<u8>,
}


/// Server for connecting clients to workers and shuttling data
/// Currently runs on a single thread - maybe look into parallelising if it doesn't scale?
pub struct JobServer {
    /// Clients connected to this server
    client_sessions: HashMap<Arc<String>, ClientDetails>,

    /// Workers connected to this server
    worker_sessions: HashMap<usize, Addr<WorkerWsSession>>,

    /// Global atomic to give workers a unique id
    worker_id_gen: AtomicUsize, // TODO: Needs to be in an Arc if using multiple server threads

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
            clients: HashMap::with_capacity(32),
            timestamp_worker: Instant::now(),
            frames_available: 0,
        };
        let null_name = null_job.config.name().unwrap();
        job_lookup.insert(null_name.clone(), null_job.wrap());
        Self {
            client_sessions: HashMap::with_capacity(64),
            worker_sessions: HashMap::with_capacity(64),
            worker_id_gen: AtomicUsize::new(0),
            job_lookup,
            unfinished_jobs: Vec::with_capacity(64),
        }
    }
}

impl Actor for JobServer {
    type Context = Context<Self>;
}

impl Handler<ClientConnect> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: ClientConnect, ctx: &mut Self::Context) -> Self::Result {
        println!("Client {} connected", msg.id);

        if let Some(old_session) = self.client_sessions.insert(msg.id.clone(), ClientDetails { addr: msg.addr, job: None }) {
            // Client started a new session before a previous one was closed,
            // so end the previous session

            // Remove interest from any previous job
            if let Some(old_job) = old_session.job {
                old_job.write().unwrap().remove_client(&msg.id);
            }

            // TODO: tell the old session actor to end its connection
            old_session.addr.do_send(ClientForceDisconnect {});
        }
    }
}

impl Handler<ClientDisconnect> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: ClientDisconnect, ctx: &mut Self::Context) -> Self::Result {
        self.client_sessions.remove(&msg.id);
        // TODO: cancel job if no clients left
    }
}

impl Handler<ClientReqJob> for JobServer {
    type Result = Job;

    fn handle(&mut self, msg: ClientReqJob, ctx: &mut Self::Context) -> Self::Result {
        // Check whether job already exists.
        // Keep job_lookup locked while we work with it to avoid races
        // (i.e. we can only add one new job at a time)
        let jobname = msg.config.name().unwrap().clone();
        let existing = self.job_lookup.get(&jobname).and_then(|j| Some(j.clone()));
        if let Some(job) = existing {
            // Attach client to job.
            let mut job_lock = job.write().unwrap();
            println!("Job with name {} already exists.", job_lock.config.name().unwrap());
            let res = job_lock.clients.insert(msg.client_id.as_str().into(), Instant::now()).is_none();
            println!("Updated timestamp for job {}", job_lock.config.name().unwrap());

            // If client wasn't already attached to that job, remove them from their old job
            if res {
                println!("Checking client's old job");
                if let Some(old_job) = msg.client_prev_job {
                    let mut old_job = old_job.write().unwrap();
                    println!("Removing client {} from old job with name {}", msg.client_id, old_job.config.name().unwrap());
                    old_job.remove_client(&msg.client_id);
                }
                println!("Finished cleaning up after {}", msg.client_id);
            } // NOTE: Assuming client_map can't get out of sync. Might need a test here if it can.
            println!("Returning job handle");
            job.clone()
        } else {
            // Create new job and attach client
            println!("Creating new job for client {}", msg.client_id);
            let mut clients = HashMap::with_capacity(32);
            let now = Instant::now();
            clients.insert(msg.client_id.as_str().into(), now);
            let new_job = JobInner {
                config: msg.config,
                status: JobStatus::Waiting,
                clients,
                timestamp_worker: now,
                frames_available: 0,
            }.wrap();
            self.job_lookup.insert(jobname, new_job.clone());
            // Add new job to list of unfinished ones
            self.unfinished_jobs.push(new_job.clone());
            if let Some(old_job) = msg.client_prev_job {
                let mut old_job = old_job.write().unwrap();
                println!("Removing client {} from old job with name {}", msg.client_id, old_job.config.name().unwrap());
                old_job.remove_client(&msg.client_id);
            }
            new_job
        }
    }
}

impl Handler<WorkerConnect> for JobServer {
    type Result = usize;

    fn handle(&mut self, msg: WorkerConnect, ctx: &mut Self::Context) -> Self::Result {
        let id = self.worker_id_gen.fetch_add(1, Ordering::SeqCst);
        println!("New worker connected, assigned {id}");

        self.worker_sessions.insert(id, msg.addr);

        id
    }
}

impl Handler<WorkerDisconnect> for JobServer {
    type Result = ();

    fn handle(&mut self, msg: WorkerDisconnect, ctx: &mut Self::Context) -> Self::Result {
        self.worker_sessions.remove(&msg.id);
    }
}


#[derive(Debug, Clone)]
pub struct JobInner {
    pub config: PytfConfig,
    pub status: JobStatus,
    pub clients: HashMap<String, Instant>, // TODO: make this a Vec<Addr<ClientWsSession>>>
    pub timestamp_worker: Instant, // TODO: remove this
    pub frames_available: usize,
}
pub type Job = Arc<RwLock<JobInner>>;

// TODO: Make worker an Addr to the WorkerWsSession
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobStatus {
    /// Waiting to be run
    Waiting,
    /// Running on the specified worker
    Running(String),
    /// Paused, last worked on by specified worker
    Paused(String),
    /// Should be stolen from the specified worker
    Steal(String),
    /// Currently being stolen from the specified worker
    Stealing(String),
    /// Job has completed
    Finished,
}

impl JobInner {
    pub fn wrap(self) -> Job {
        Arc::new(RwLock::new(self))
    }

    pub fn remove_client(&mut self, client: &str) {
        self.clients.remove(client);
        if self.clients.is_empty() {
            println!("Job with name {} is now empty.", self.config.name().unwrap());
            self.status = match &self.status {
                JobStatus::Waiting => JobStatus::Waiting,
                JobStatus::Finished => JobStatus::Finished,

                JobStatus::Running(worker) => {
                    self.signal_pause(&worker);
                    JobStatus::Paused(worker.clone())
                },
                JobStatus::Paused(worker) => JobStatus::Paused(worker.clone()),

                // TODO: after stealing a job, worker should check whether
                // that job is now paused, in which case the Paused(worker) is updated
                JobStatus::Stealing(worker) => JobStatus::Paused(worker.clone()),
                JobStatus::Steal(worker) => JobStatus::Paused(worker.clone()),
            }
        }
    }

    pub fn signal_pause(&self, worker: &str) {
        let jobname = self.config.name().unwrap();
        println!("Sending stop signal to {worker} for {jobname}");
        actix_web::rt::spawn(
            Client::default()
                .post(format!("{worker}/stop/{jobname}"))
                .send_json(&JobSignalPause::from(jobname.clone()))
        );
    }
}





#[derive(Debug, Clone)]
pub struct JobQueue {
    /// Main job storage, indexed by job name (which is unique)
    job_lookup: Arc<Mutex<HashMap<String, Job>>>,
    /// Map of which client cares about which job. Job in Mutex so it can be switched out
    client_map: Arc<RwLock<HashMap<String, Arc<Mutex<Option<Job>>>>>>,
    /// List of unfinished jobs - candidates for work requests
    unfinished_jobs: Arc<RwLock<Vec<Job>>>,
}

#[derive(Debug, Clone)]
pub struct JobAssignment {
    /// Config to run
    job: Job,
    /// Worker to steal previous results from
    steal_from: Option<String>,
}

impl JobAssignment {
    pub fn send(self, worker: String) {
        let message: JobForWorker = self.into();
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JobForWorker {
    config: PytfConfig,
    steal_from: Option<String>,
}

impl From<JobAssignment> for JobForWorker {
    fn from(value: JobAssignment) -> Self {
        Self {
            config: value.job.read().unwrap().config.clone(),
            steal_from: value.steal_from,
        }
    }
}

pub const STEAL_TIMEOUT: Duration = Duration::from_secs(30);

impl JobQueue {
    pub fn new() -> Self {
        let null_job = JobInner {
            config: PytfConfig::default(),
            status: JobStatus::Finished,
            clients: HashMap::with_capacity(32),
            timestamp_worker: Instant::now(),
            frames_available: 0,
        };
        let null_name = null_job.config.name().unwrap();
        let mut job_lookup = HashMap::with_capacity(128);
        job_lookup.insert(null_name.clone(), null_job.wrap());
        Self {
            job_lookup: Arc::new(Mutex::new(job_lookup)),
            client_map: Arc::new(RwLock::new(HashMap::with_capacity(64))),
            unfinished_jobs: Arc::new(RwLock::new(Vec::with_capacity(128))),
        }
    }

    pub fn notify_workers(&self) {
        // TODO: Send signal (via web sockets?) that a new job is available
    }

    /// Find a job for a worker and assign it if one is found.
    /// Returns a copy of the configuration (to be forwarded to the worker)
    /// if a job is found, or `None` if there are no avialable jobs.
    pub fn assign_worker(&self, worker: String) -> Option<JobAssignment> {
        for j in self.unfinished_jobs.read().unwrap().iter() {
            let mut job = j.write().unwrap();
            if job.clients.len() > 0 {
                match &job.status {
                    JobStatus::Waiting => {
                        job.status = JobStatus::Running(worker);
                        job.timestamp_worker = Instant::now();
                        return Some(JobAssignment { job: j.clone(), steal_from: None });
                    },
                    JobStatus::Steal(old_worker) => {
                        let old_worker = old_worker.clone();
                        job.status = JobStatus::Stealing(old_worker.clone());
                        job.timestamp_worker = Instant::now();
                        return Some(JobAssignment { job: j.clone(), steal_from: Some(old_worker) });
                    },
                    JobStatus::Stealing(old_worker) => {
                        let now = Instant::now();
                        if now.duration_since(job.timestamp_worker) > STEAL_TIMEOUT {
                            let old_worker = old_worker.clone();
                            eprintln!("Timeout on stealing job from worker {old_worker}. Reassigning to {worker}.");
                            // Status unchanged since still stealing from the same worker
                            job.timestamp_worker = Instant::now();
                            return Some(JobAssignment {
                                job: j.clone(),
                                steal_from: Some(old_worker),
                            });
                        }
                    },
                    _ => (),
                }
            }
        }
        None
    }

    /// Request from a given `client` that a job be added to the queue.
    /// If the client was previously interested in a different job, that
    /// interest is cleared. Any job that ends up with no remaining interest
    /// will have a cancel signal sent to the attached worker.
    /// Returns `Some(new_job)` if a new job is added, or `None` if there was
    /// already a matching existing job.
    pub fn request_job(&self, config: PytfConfig, client: String) -> Option<Job> {
        // Get the job currently mapped to the client, or create one
        // if we haven't seen this client before
        let client_job = {
            // Add client to lookup if they're not already in it.
            // Need to use write() lock to avoid potential race
            // between dropping a read() lock after checking existence
            // and creating a write() lock to insert
            let mut client_map = self.client_map.write().unwrap();
            println!("Client map is {client_map:?}");
            if !client_map.contains_key(&client) {
                println!("Adding client {client} to map with blank job");
                let no_job = Arc::new(Mutex::new(None));
                client_map.insert(client.clone(), no_job.clone());
                no_job
            } else {
                println!("Client is known, retrieving previous job");
                client_map.get(&client).unwrap().clone()
            }
        };
        // Check whether job already exists.
        // Keep job_lookup locked while we work with it to avoid races
        // (i.e. we can only add one new job at a time)
        let jobname = config.name().unwrap().clone();
        let mut lock_existing_jobs = self.job_lookup.lock().unwrap();
        let existing = lock_existing_jobs.get(&jobname).and_then(|j| Some(j.clone()));
        if let Some(job) = existing {
            // Attach client to job.
            let mut job_lock = job.write().unwrap();
            println!("Job with name {} already exists.", job_lock.config.name().unwrap());
            let res = job_lock.clients.insert(client.clone(), Instant::now()).is_none();

            // If client wasn't already attached to that job, remove them from their old job
            if res {
                println!("Client {client} coming from different job. Attaching to new one.");
                let old_job = { client_job.lock().unwrap().replace(job.clone()) };
                if let Some(old_job) = old_job {
                    let mut old_job = old_job.write().unwrap();
                    println!("Removing client {client} from old job with name {}", old_job.config.name().unwrap());
                    old_job.remove_client(&client);
                }
                println!("Finished cleaning up after {client}");
            } // NOTE: Assuming client_map can't get out of sync. Might need a test here if it can.
            None
        } else {
            // Create new job and attach client
            println!("Creating new job for client {client}");
            let mut clients = HashMap::with_capacity(32);
            let now = Instant::now();
            clients.insert(client.clone(), now);
            let new_job = JobInner {
                config,
                status: JobStatus::Waiting,
                clients,
                timestamp_worker: now,
                frames_available: 0,
            }.wrap();
            lock_existing_jobs.insert(jobname, new_job.clone());
            // Add new job to list of unfinished ones
            self.unfinished_jobs.write().unwrap().push(new_job.clone());
            let old_job = { client_job.lock().unwrap().replace(new_job.clone()) };
            if let Some(old_job) = old_job {
                let mut old_job = old_job.write().unwrap();
                println!("Removing client {client} from old job with name {}", old_job.config.name().unwrap());
                old_job.remove_client(&client);
            }
            Some(new_job)
        }
        // TODO: return web socket details based on job_ref?? Or client just asks for more frames?
    }

    pub fn cancel_job(&self, client: &str) -> bool {
        let client_job = { self.client_map.read().unwrap().get(client).cloned() };
        // Make sure client was known
        if let Some(client_job) = client_job {
            println!("Client {client} is known.");
            // Make sure client has a current job
            if let Some(job) = client_job.lock().unwrap().take() {
                println!("Cancelling job for client {client}");
                job.write().unwrap().remove_client(client);
                return true
            }
        }
        false
    }
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JobSignalPause {
    jobname: String,
}
impl From<String> for JobSignalPause {
    fn from(jobname: String) -> Self {
        Self { jobname }
    }
}

