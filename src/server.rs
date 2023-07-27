use actix_cors::Cors;
use actix_files::{Files, NamedFile};
use actix_identity::{Identity, IdentityMiddleware};
use actix_session::{storage::RedisSessionStore, SessionMiddleware};
use actix_web::{
    cookie::Key, http, post, web, App, HttpMessage, HttpRequest, HttpResponse, HttpServer,
    Responder, dev::Server
};
use crate::authentication::{self, UserDB, LoginToken, UserCredentials};

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

pub async fn server() -> std::io::Result<Server> {
    // Load user database
    let _ = authentication::USER_DB.set(UserDB::from_cli_or_default(std::env::args()));

    let address = "127.0.0.1";
    let port = 8080;
    let redis_port = 6379;

    let secret_key = Key::generate();
    let redis_address = format!("redis://127.0.0.1:{redis_port}");
    let redis_store = RedisSessionStore::new(redis_address)
        .await
        .expect("Could not connect to redis-server");

    Ok(HttpServer::new(move || {
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
            .service(Files::new("/", FRONTEND_ROOT))
    })
    .bind((address, port))?
    .run())
}
