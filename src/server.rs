use std::{collections::{HashMap, HashSet}, sync::{RwLock, Arc, Mutex}, time::Duration};

use actix::{clock::Instant, Actor, Context, Handler, Message};
use actix_cors::Cors;
use actix_files::{Files, NamedFile};
use actix_identity::{Identity, IdentityMiddleware};
use actix_session::{storage::RedisSessionStore, SessionMiddleware};
use actix_web::{
    cookie::Key, http, get, post, web, App, HttpMessage, HttpRequest, HttpResponse, HttpServer,
    Responder
};

use awc::Client;

use pytf_web::{authentication::{self, UserDB, LoginToken, UserCredentials}, pytf_config::{PytfConfig, AVAILABLE_MOLECULES, MixtureComponentDetailed, MoleculeResources}};

const FRONTEND_ROOT: &'static str = "./pytf-viewer/build";

async fn index() -> impl Responder {
    NamedFile::open_async(format!("{FRONTEND_ROOT}/index.html"))
        .await
        .expect("Could not find index.html! Make sure pytf-viewer has been built (npm run build)")
}

#[post("/login")]
async fn login(request: HttpRequest, credentials: web::Json<UserCredentials>) -> impl Responder {
    if !authentication::USER_DB
        .get()
        .unwrap()
        .validate_user(&credentials)
    {
        return HttpResponse::Unauthorized().body("Incorrect username or password.");
    }

    match Identity::login(&request.extensions(), credentials.get_id()) {
        Ok(user) => {
            let user_id = user.id().unwrap();
            println!("Logged in ({user_id})");
            HttpResponse::Ok().json(LoginToken::from(user_id))
        }
        Err(e) => HttpResponse::ExpectationFailed().body(format!("{e}")),
    }
}

#[post("/logout")]
async fn logout(user: Identity) -> impl Responder {
    let user_id = user.id().unwrap();
    println!("Logged out ({user_id})");
    user.logout();
    HttpResponse::Ok()
}

#[post("/user-token")]
async fn user_token(user: Identity) -> impl Responder {
    let user_id = user.id().unwrap();
    println!("Sending cached token ({})", user_id);
    HttpResponse::Ok().json(LoginToken::from(user_id))
}

#[post("/submit")]
async fn submit(user: Identity, mut config: web::Json<PytfConfig>, job_queue: web::Data<JobQueue>) -> impl Responder {
    println!("Received config: {config:?}");
    config.canonicalize();
    config.prefill();
    // TODO:
    // * Look up settings.name in hash map to get attached worker or choose a new one.
    // * Send back worker's address
    // * Keep track of which worker is working on what, and with what status (allocated, working, done)

    println!("Processed config: {config:?}");
    job_queue.request_job(config.into_inner(), user.id().unwrap());

    HttpResponse::Ok() // TODO: Send web socket info? Or job name?
}

#[post("/cancel")]
async fn cancel(user: Identity, job_queue: web::Data<JobQueue>) -> impl Responder {
    let client = user.id().unwrap();
    println!("Got cancel signal from {client}");
    // Get handle to user's job, leaving queue unlocked afterwards
    let client_job = { job_queue.client_map.read().unwrap().get(&client).cloned() };
    // Make sure client was known
    if let Some(client_job) = client_job {
        println!("Client {client} is known.");
        // Make sure client has a current job
        if let Some(job) = client_job.lock().unwrap().take() {
            println!("Cancelling job for client {client}");
            job.lock().unwrap().remove_client(&client);
            return HttpResponse::Ok().finish()
        }
    }
    HttpResponse::NotModified().body("No job in queue for client")

}

#[get("/molecules")]
async fn molecules(_user: Identity) -> impl Responder {
    HttpResponse::Ok().json(AVAILABLE_MOLECULES.get())
}


#[get("/get_work")]
async fn get_work(user: Identity, job_queue: web::Data<JobQueue>) -> impl Responder {
    // TODO:
    //  * Allow workers to log in with a "worker" username
    //  * Set their id to their address
    //  * Store list of registered workers, check against it here before proceeding.
    job_queue.assign_worker(user.id().unwrap());
    HttpResponse::Ok()
}


struct WorkerAllocation {
    /// Keep track of which job each worker is working on
    workers: RwLock<HashMap<String, Option<Job>>>,
}

impl WorkerAllocation {
    pub fn new() -> Self {
        let mut args = std::env::args();
        let mut workers = None;
        while let Some(arg) = args.next() {
            if arg == "-w" || arg == "--workers" {
                let mut worker_hash = HashMap::with_capacity(args.len());
                while let Some(worker) = args.next() {
                    if worker_hash.insert(worker.clone(), None).is_some() {
                        eprintln!("WARNING: Got duplicate worker address: {worker}");
                    }
                }
                workers = Some(worker_hash);
                break;
            }
        }
        let workers = workers.unwrap_or_else(|| {
            eprintln!("WARNING: No worker addresses specified!");
            HashMap::new()
        });
        Self { workers: RwLock::new(workers) }
    }

}

#[derive(Clone, Debug, Eq, PartialEq)]
enum JobStatus {
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

#[derive(Debug, Clone)]
struct JobInner {
    config: PytfConfig,
    status: JobStatus,
    clients: HashMap<String, Instant>,
    timestamp_worker: Instant,
}
type Job = Arc<Mutex<JobInner>>;

impl JobInner {
    pub fn wrap(self) -> Job {
        Arc::new(Mutex::new(self))
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
struct JobQueue {
    /// Main job storage, indexed by job name (which is unique)
    job_lookup: Arc<Mutex<HashMap<String, Job>>>,
    /// Map of which client cares about which job. Job in Mutex so it can be switched out
    client_map: Arc<RwLock<HashMap<String, Arc<Mutex<Option<Job>>>>>>,
    /// List of unfinished jobs - candidates for work requests
    unfinished_jobs: Arc<RwLock<Vec<Job>>>,
}

#[derive(Debug, Clone)]
struct JobAssignment {
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
struct JobForWorker {
    config: PytfConfig,
    steal_from: Option<String>,
}

impl From<JobAssignment> for JobForWorker {
    fn from(value: JobAssignment) -> Self {
        Self {
            config: value.job.lock().unwrap().config.clone(),
            steal_from: value.steal_from,
        }
    }
}

pub const STEAL_TIMEOUT: Duration = Duration::from_secs(30);

impl JobQueue {
    fn new() -> Self {
        let null_job = JobInner {
            config: PytfConfig::default(),
            status: JobStatus::Finished,
            clients: HashMap::with_capacity(32),
            timestamp_worker: Instant::now(),
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

    /// Find a job for a worker and assign it if one is found.
    /// Returns a copy of the configuration (to be forwarded to the worker)
    /// if a job is found, or `None` if there are no avialable jobs.
    fn assign_worker(&self, worker: String) -> Option<JobAssignment> {
        for j in self.unfinished_jobs.read().unwrap().iter() {
            let mut job = j.lock().unwrap();
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
    fn request_job(&self, config: PytfConfig, client: String) {
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
                println!("Client is known, retrieving previous job.");
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
            let mut job_lock = job.lock().unwrap();
            println!("Job with name {} already exists.", job_lock.config.name().unwrap());
            let res = job_lock.clients.insert(client.clone(), Instant::now()).is_none();

            // If client wasn't already attached to that job, remove them from their old job
            if res {
                println!("Client {client} coming from different job. Attaching to new one.");
                let old_job = { client_job.lock().unwrap().replace(job.clone()) };
                if let Some(old_job) = old_job {
                    let mut old_job = old_job.lock().unwrap();
                    println!("Removing client {client} from old job with name {}", old_job.config.name().unwrap());
                    old_job.remove_client(&client);
                }
                println!("Finished cleaning up after {client}");
            } // NOTE: Assuming client_map can't get out of sync. Might need a test here if it can.
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
            }.wrap();
            lock_existing_jobs.insert(jobname, new_job.clone());
            // Add new job to list of unfinished ones
            self.unfinished_jobs.write().unwrap().push(new_job.clone());
            let old_job = { client_job.lock().unwrap().replace(new_job) };
            if let Some(old_job) = old_job {
                let mut old_job = old_job.lock().unwrap();
                println!("Removing client {client} from old job with name {}", old_job.config.name().unwrap());
                old_job.remove_client(&client);
            }
        }
        // TODO: return web socket details based on job_ref?? Or client just asks for more frames?
        println!("Exiting request_job");
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct JobSignalPause {
    jobname: String,
}
impl From<String> for JobSignalPause {
    fn from(jobname: String) -> Self {
        Self { jobname }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load user database
    let _ = authentication::USER_DB.set(UserDB::from_cli_or_default(std::env::args()));

    // Load list of available molecules
    let _ = AVAILABLE_MOLECULES.set(MoleculeResources::from_cli_or_default(std::env::args()));

    let address = "127.0.0.1";
    let port = 8080;
    let redis_port = 6379;

    let secret_key = Key::generate();
    let redis_address = format!("redis://127.0.0.1:{redis_port}");
    let redis_store = RedisSessionStore::new(redis_address)
        .await
        .expect("Could not connect to redis-server");

    // TODO: - Read in list of worker addresses/ports
    //       - Set up RwLock<HashMap<Worker, String>> for keeping track of which config runs where
    //       - Forward on simulation requests and return worker address for web socket connection

    let worker_allocation = web::Data::new(WorkerAllocation::new());
    let job_queue = web::Data::new(JobQueue::new());

    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin(format!("http://localhost:{port}").as_str())
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec![
                http::header::AUTHORIZATION,
                http::header::ACCEPT,
                http::header::CONTENT_TYPE,
            ])
            .max_age(3600);
        App::new()
            .app_data(worker_allocation.clone())
            .app_data(job_queue.clone())
            .wrap(
                IdentityMiddleware::builder()
                    .visit_deadline(Some(std::time::Duration::from_secs(24 * 60 * 60)))
                    .build(),
            )
            // SessionMiddleware must be mounted *after* IdentityMiddleware
            .wrap(SessionMiddleware::new(
                redis_store.clone(),
                secret_key.clone(),
            ))
            .wrap(cors)
            .service(web::resource("/").to(index))
            .service(login)
            .service(logout)
            .service(user_token)
            .service(submit)
            .service(cancel)
            .service(molecules)
            .service(Files::new("/", FRONTEND_ROOT))
    })
    .bind((address, port))?
    .run()
    .await
}
