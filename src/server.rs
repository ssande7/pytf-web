use std::{collections::HashMap, sync::RwLock};

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
async fn submit(_user: Identity, mut config: web::Json<PytfConfig>, workers: web::Data<RwLock<WorkerAllocation>>) -> impl Responder {
    println!("Received config: {config:?}");
    config.canonicalize();
    config.prefill();
    // TODO:
    // * Look up settings.name in hash map to get attached worker or choose a new one.
    // * Send back worker's address
    // * Keep track of which worker is working on what, and with what status (allocated, working, done)

    println!("Processed config: {config:?}");

    let existing_job = {
        workers.read().expect("workers mutex poisoned!")
            .get_job(&config)
            .and_then(|j| Some((j.worker.clone(), j.status)))
    };

    // TODO: Make default empty job send back just the graphene base?

    let worker =
    if let Some((worker, status)) = existing_job {
        match status {
            JobStatus::Allocated
                |JobStatus::Running
                | JobStatus::Complete
                => worker,
            JobStatus::Failed
                => return HttpResponse::InternalServerError().body("Unknown job failure. Try some different input parameters."),
            JobStatus::Idle
                => unimplemented!("Move job to another worker and connect the user"),
        }
    } else if let Some(worker) = {
        workers.write().unwrap().add_job(config.into_inner()).await
    } {
        worker
    } else {
        eprintln!("Failed to find a worker");
        return HttpResponse::InternalServerError().body("No workers available! Try again later.");
    };
    println!("Selected worker {worker}");

    HttpResponse::Ok().body(worker)
}

#[post("/cancel")]
async fn cancel(_user: Identity) -> impl Responder {
    // TODO: Cancel the job currently being run by the user
    HttpResponse::Ok()
}

#[get("/molecules")]
async fn molecules(_user: Identity) -> impl Responder {
    HttpResponse::Ok().json(AVAILABLE_MOLECULES.get())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkerStatus {
    /// Worker is currently idle
    Idle,
    /// Worker is allocated to job with given name, but hasn't reported starting it yet
    Allocated(String),
    /// Worker is currently running job with given name
    Running(String),
}

#[derive(Debug, Clone, Copy)]
enum JobStatus {
    /// Job has been allocated to a worker
    Allocated,
    /// Job is currently running
    Running,
    /// Job has completed successfully
    Complete,
    /// Job was partly run, but stopped before completion
    Idle,
    /// Job failed with an error
    Failed,
}

#[derive(Debug, Clone)]
struct Simulation {
    config: PytfConfig,
    worker: String,
    status: JobStatus,
}

impl PartialEq<PytfConfig> for Simulation {
    fn eq(&self, other: &PytfConfig) -> bool {
        other == &self.config
    }
    fn ne(&self, other: &PytfConfig) -> bool {
        other != &self.config
    }
}

// TODO: Worker keeps track of attached users, reports back when no users left.

struct WorkerAllocation {
    /// Keep track of which job each worker is working on
    workers: HashMap<String, WorkerStatus>,
    jobs: HashMap<String, Simulation>,
}

impl WorkerAllocation {
    pub fn new() -> Self {
        let mut args = std::env::args();
        let mut workers = None;
        while let Some(arg) = args.next() {
            if arg == "-w" || arg == "--workers" {
                let mut worker_hash = HashMap::with_capacity(args.len());
                while let Some(worker) = args.next() {
                    if worker_hash.insert(worker.clone(), WorkerStatus::Idle).is_some() {
                        eprintln!("Got duplicate worker address: {worker}");
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
        Self {
            workers,
            jobs: HashMap::with_capacity(128),
        }
    }

    pub fn get_job(&self, config: &PytfConfig) -> Option<&Simulation> {
        let Some(jobname) = config.name() else { return None };
        self.jobs.get(jobname)
    }

    pub async fn add_job(&mut self, config: PytfConfig) -> Option<String> {
        println!("Adding job...");
        let Some(jobname) = config.name() else {
            eprintln!("WARNING: Called add_job with uninitialised config");
            return None;
        };
        let jobname = jobname.clone();

        let Some((idle_worker, _)) = self.workers.iter().find(|&(_, status)| {
            *status == WorkerStatus::Idle
        }) else { eprintln!("No workers available!"); return None };
        let idle_worker = idle_worker.clone();
        println!("Found idle worker: {idle_worker}");

        // Mark worker as assigned to this job
        *self.workers.get_mut(&idle_worker).unwrap() = WorkerStatus::Allocated(jobname.clone());

        // TODO: Send message to worker to tell it to start this job.
        //       Thread holds a write() lock at this point, so other threads will
        //       wait until this job has started before they can check for/start other jobs.
        //       Maybe more efficient to send config after this exits so other job requests to the
        //       server can be handled while this one is starting up? Make sure that doesn't
        //       introduce a race first...

        // TODO:  Instead of directly sending to worker, just add job to a queue for workers to
        //        pull from (while registering themselves for that job)?
        //        Workers send back job results as they produce them (with a retry system?)
        let worker_client = Client::default();
        let res = worker_client
                .put(format!("{idle_worker}/deposit"))
                .send_json(&config)
                .await;
        // TODO: check res for success
        println!("Submitted job to worker. Result: {res:?}");


        self.jobs.insert(
            jobname,
            Simulation {
                config,
                worker: idle_worker.clone(),
                status: JobStatus::Allocated
            });
        // TODO: Arc jobname to avoid so many clones?


        return Some(idle_worker)
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
            .app_data(web::Data::new(RwLock::new(WorkerAllocation::new())))
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
