use actix_files::{Files, NamedFile};
use actix_web::{web, http, App, HttpServer, Responder};
use actix_cors::Cors;

async fn index() -> impl Responder {
    NamedFile::open_async("./pytf-viewer/build/index.html").await.unwrap()
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let address = "127.0.0.1";
    let port = 8080;
    HttpServer::new(|| {
        let cors = Cors::default()
            .allowed_origin("http://localhost:8080")
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec![http::header::AUTHORIZATION, http::header::ACCEPT, http::header::CONTENT_TYPE])
            .max_age(3600);
            
        App::new()
            .wrap(cors)
            .service(web::resource("/").to(index))
            .service(Files::new("/", "./pytf-viewer/build"))
    })
    .bind((address, port))?
    .run()
    .await
}
