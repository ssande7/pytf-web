use anyhow::anyhow;
use actix_web::web::Bytes;
use awc::ws;
use pytf_web::{pytf_frame::{AtomNameMap, ATOM_NAME_MAP}, worker_client::WsMessage};

use pytf_web::worker_client::PytfServer;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {

    // Expect server address as first command line argument and worker key as second
    let mut args = std::env::args().skip(1);
    let Some(server_addr) = args.next() else {
        return Err(anyhow!("Expected two positional arguments for server address and key string"));
    };
    let Some(key) = args.next() else {
        return Err(anyhow!("Missing key string argument"));
    };

    // Load atom name map
    let _ = ATOM_NAME_MAP.set(AtomNameMap::from_cli_or_default(std::env::args()));

    // Set up connection to server. Server must be available or this will fail.
    let pytf_server = PytfServer::connect(server_addr, key).await?;
    println!("Sending test ping");
    pytf_server.do_send(WsMessage(ws::Message::Ping(Bytes::from_static(b""))));

    let _ = actix_rt::signal::ctrl_c().await?;
    Ok(())
}





