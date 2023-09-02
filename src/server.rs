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

use pytf_web::{
    authentication::{self, UserDB, LoginToken, UserCredentials},
    pytf_config::{AVAILABLE_MOLECULES, MoleculeResources},
    pytf_frame::WS_FRAME_SIZE_LIMIT
};

const FRONTEND_ROOT: &'static str = "./pytf-viewer/build";

async fn index() -> impl Responder {
    NamedFile::open_async(format!("{FRONTEND_ROOT}/index.html"))
        .await
        .expect("Could not find index.html! Make sure pytf-viewer has been built (npm run build)")
}

#[post("/login")]
async fn login(request: HttpRequest, credentials: web::Json<UserCredentials>) -> impl Responder {
    println!("Received login request with credentials: {credentials:?}");
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
    let uid = user.id().unwrap();
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
            .service(molecules)
            .service(Files::new("/", FRONTEND_ROOT))
    })
    .bind((address, port))?
    .run()
    .await
}
