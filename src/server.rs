use std::{collections::HashMap, sync::RwLock};

use actix::{Addr, Actor};
use actix_cors::Cors;
use actix_files::{Files, NamedFile};
use actix_identity::{Identity, IdentityMiddleware};
use actix_session::{storage::RedisSessionStore, SessionMiddleware};
use actix_web::{
    cookie::Key, http, get, post, web, App, HttpMessage, HttpRequest, HttpResponse, HttpServer,
    Responder
};

mod job_queue;
use actix_web_actors::ws;
use client_session::ClientWsSession;
use job_queue::*;

mod client_session;
mod worker_session;

use pytf_web::{
    authentication::{self, UserDB, LoginToken, UserCredentials},
    pytf_config::{PytfConfig, AVAILABLE_MOLECULES, MoleculeResources}
};

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

#[get("/socket")]
async fn socket(
    req: HttpRequest,
    user: Identity,
    stream: web::Payload,
    srv: web::Data<Addr<JobServer>>,
) -> Result<HttpResponse, actix_web::Error> {
    ws::start(
        ClientWsSession::new(
            user.id().unwrap(),
            srv.get_ref().clone()
        ),
        &req,
        stream,
    )
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
    if let Some(_) = job_queue.request_job(config.into_inner(), user.id().unwrap()) {
        job_queue.notify_workers();
    }

    HttpResponse::Ok() // TODO: Send web socket info? Or job name?
}

#[post("/cancel")]
async fn cancel(user: Identity, job_queue: web::Data<JobQueue>) -> impl Responder {
    let client = user.id().unwrap();
    println!("Got cancel signal from {client}");
    if job_queue.cancel_job(&user.id().unwrap()) {
        return HttpResponse::Ok().finish()
    } else {
        HttpResponse::NotModified().body("No job in queue for client")
    }

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

    let job_server = JobServer::new().start();

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
            // .app_data(worker_allocation.clone())
            // .app_data(job_queue.clone())
            .app_data(web::Data::new(job_server.clone()))
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
            .service(socket)
            .service(submit)
            .service(cancel)
            .service(molecules)
            .service(Files::new("/", FRONTEND_ROOT))
    })
    .bind((address, port))?
    .run()
    .await
}
