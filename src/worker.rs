use anyhow::anyhow;
use pytf_web::pytf_frame::{AtomNameMap, ATOM_NAME_MAP};

use pytf_web::worker_client::PytfServer;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // Expect server address as first command line argument and worker key as second
    let mut args = std::env::args().skip(1);
    let Some(server_addr) = args.next() else {
        return Err(anyhow!("Expected two positional arguments for server address and key string"));
    };
    let Some(key) = args.next() else {
        return Err(anyhow!("Missing key string argument"));
    };

    // Generate atom name map
    let _ = ATOM_NAME_MAP.set(AtomNameMap::create());

    // Set up connection to server. Will retry if server is unavailable or connection fails.
    let _ = PytfServer::connect(server_addr, key).await;

    let _ = actix_rt::signal::ctrl_c().await?;
    Ok(())
}

