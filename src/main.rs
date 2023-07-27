mod pytf;
mod pytf_config;
mod pytf_runner;
mod worker;
mod authentication;
mod server;

use server::server;
use worker::worker;

enum HostType {
    Server,
    Worker,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let conf = HostType::Server;
    match conf {
        HostType::Server => server().await?.await?,
        HostType::Worker => worker()?.await?,
    }
    Ok(())
}
