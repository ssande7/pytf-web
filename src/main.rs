use actix_cors::Cors;
use actix_files::{Files, NamedFile};
use actix_identity::{Identity, IdentityMiddleware};
use actix_session::{storage::RedisSessionStore, SessionMiddleware};
use actix_web::{
    cookie::Key, http, post, web, App, HttpMessage, HttpRequest, HttpResponse, HttpServer,
    Responder,
};
use serde::{Serialize, Deserialize};

const FRONTEND_ROOT: &'static str = "./pytf-viewer/build";

async fn index() -> impl Responder {
    NamedFile::open_async(format!("{FRONTEND_ROOT}/index.html"))
        .await
        .expect("Could not find index.html! Make sure pytf-viewer has been built (npm run build)")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoginToken {
    token: String,
}

#[derive(Debug, Clone, Deserialize)]
struct UserCredentials {
    username: String,
    // TODO: should this be received as a hashed password?
    password: String,
}

#[post("/login")]
async fn login(request: HttpRequest, credentials: web::Json<UserCredentials>) -> impl Responder {
    // TODO: authentication happens here
    match Identity::login(&request.extensions(), credentials.username.clone()) {
        Ok(user) => {
            println!("Logged in");
            HttpResponse::Ok().json(LoginToken { token: user.id().unwrap().clone() })
        }
        Err(e) => HttpResponse::ExpectationFailed().body(format!("{e}")),
    }
}

#[post("/logout")]
async fn logout(user: Identity) -> impl Responder {
    println!("Logged out");
    user.logout();
    HttpResponse::Ok()
}

#[post("/user-token")]
async fn user_token(user: Identity) -> impl Responder {
    println!("Sending cached token ({})", user.id().unwrap());
    HttpResponse::Ok().json(LoginToken { token: user.id().unwrap() })
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let address = "127.0.0.1";
    let port = 8080;
    let redis_port = 6379;

    let secret_key = Key::generate();
    let redis_address = format!("redis://127.0.0.1:{redis_port}");
    let redis_store = RedisSessionStore::new(redis_address)
        .await
        .expect("Could not connect to redis-server");

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
    .run()
    .await
}
