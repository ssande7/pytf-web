use std::io::{Error, ErrorKind};

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
use job_queue::*;

mod client_session;
use client_session::ClientWsSession;
mod worker_session;
use worker_session::WorkerWsSession;

mod server_args;
use server_args::parse_args;

use pytf_web::{
    authentication::{self, LoginToken, UserCredentials},
    pytf_config::AVAILABLE_MOLECULES,
    pytf_frame::WS_FRAME_SIZE_LIMIT
};

const FRONTEND_ROOT: &'static str = "./pytf-viewer/build";

async fn index(request: HttpRequest) -> impl Responder {
    match NamedFile::open_async(format!("{FRONTEND_ROOT}/index.html")).await {
        Ok(file) => file.respond_to(&request),
        Err(e) => {
            log::error!("Error fetching index.html: {e}");
            log::warn!("Make sure pytf-viewer has been built (npm run build)");
            HttpResponse::NotFound().finish()
        }
    }
}

#[post("/login")]
async fn login(request: HttpRequest, credentials: web::Json<UserCredentials>) -> impl Responder {
    log::debug!("Received login request.");
    if !authentication::USER_DB
        .get()
        .unwrap()
        .validate_user(&credentials)
    {
        return HttpResponse::Unauthorized().body("Incorrect username or password.");
    }

    match Identity::login(&request.extensions(), credentials.get_id()) {
        Ok(user) => {
            let Ok(user_id) = user.id() else {
                log::error!("Failed to get id from user identity.");
                return HttpResponse::InternalServerError().body("User session corrupted.")
            };
            log::info!("Logged in ({user_id})");
            HttpResponse::Ok().json(LoginToken::from(user_id))
        }
        Err(e) => HttpResponse::ExpectationFailed().body(format!("{e}")),
    }
}

#[post("/logout")]
async fn logout(user: Identity) -> impl Responder {
    let Ok(user_id) = user.id() else {
        log::error!("Failed to get id from user identity.");
        return HttpResponse::InternalServerError().body("User session corrupted.")
    };
    log::info!("Logged out ({user_id})");
    user.logout();
    HttpResponse::Ok().finish()
}

#[post("/user-token")]
async fn user_token(user: Identity) -> impl Responder {
    let Ok(user_id) = user.id() else {
        log::error!("Failed to get id from user identity.");
        return HttpResponse::InternalServerError().body("User session corrupted.")
    };
    log::info!("Sending cached token ({})", user_id);
    HttpResponse::Ok().json(LoginToken::from(user_id))
}

#[get("/socket")]
async fn socket(
    req: HttpRequest,
    user: Identity,
    stream: web::Payload,
    srv: web::Data<Addr<JobServer>>,
) -> Result<HttpResponse, actix_web::Error> {
    let Ok(uid) = user.id() else {
        return Err(Error::new(ErrorKind::InvalidData, "User session corrupted.").into())
    };
    if uid == "worker" {
        ws::WsResponseBuilder::new(
            WorkerWsSession::new(srv.get_ref().clone()),
            &req,
            stream,
        ).frame_size(WS_FRAME_SIZE_LIMIT).start()
    } else {
        ws::WsResponseBuilder::new(
            ClientWsSession::new(uid, srv.get_ref().clone()),
            &req,
            stream,
        ).frame_size(WS_FRAME_SIZE_LIMIT).start()
    }
}

#[get("/molecules")]
async fn molecules(_user: Identity) -> impl Responder {
    HttpResponse::Ok().json(AVAILABLE_MOLECULES.get())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let Some((server, redis)) = (match parse_args() {
        Ok(addr) => addr,
        Err(e) => {
            return Err(Error::new(ErrorKind::InvalidInput,
                format!("Error parsing command line arguments: {e}")));
        }
    }) else { return Ok(()) };


    let secret_key = Key::generate();
    let redis_store = RedisSessionStore::new(format!("redis://{}:{}", redis.address, redis.port))
        .await
        .expect("Could not connect to redis-server");

    let job_server = JobServer::new().start();

    HttpServer::new(move || {
        let cors = Cors::default()
            // TODO: Can we make this more restrictive?
            // .allowed_origin(format!("http://localhost:{port}").as_str())
            // .allowed_origin(format!("http://127.0.0.1:{port}").as_str())
            .allow_any_origin()
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec![
                http::header::AUTHORIZATION,
                http::header::ACCEPT,
                http::header::CONTENT_TYPE,
            ])
            .max_age(3600);
        App::new()
            .app_data(web::Data::new(job_server.clone()))
            .wrap(
                IdentityMiddleware::builder()
                    .visit_deadline(Some(std::time::Duration::from_secs(24 * 60 * 60)))
                    .build(),
            )
            // SessionMiddleware must be mounted *after* IdentityMiddleware
            .wrap(
                SessionMiddleware::builder(
                    redis_store.clone(),
                    secret_key.clone(),
                )
                .cookie_secure(false)
                .build()
            )
            .wrap(cors)
            .service(web::resource("/").to(index))
            .service(login)
            .service(logout)
            .service(user_token)
            .service(socket)
            .service(molecules)
            .service(Files::new("/", FRONTEND_ROOT))
    })
    .bind((server.address, server.port))?
    .run()
    .await
}
