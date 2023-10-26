use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::anyhow;
use pytf_web::pytf_config::{RESOURCES_DIR, WORK_DIR};
use pytf_web::pytf_frame::{AtomNameMap, ATOM_NAME_MAP};

use pytf_web::worker_client::PytfWorker;

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

    let mut resources = None;
    let mut work_dir = None;

    while let Some(arg) = args.next() {
        match arg.as_ref() {
            "-r" | "--resources" => {
                resources = args.next();
                if resources.is_none() {
                    return Err(anyhow!("Missing argument for resources directory"));
                }
            }
            "-w" | "--work-dir" => {
                work_dir = args.next();
                if work_dir.is_none() {
                    return Err(anyhow!("Missing argument for working directory"));
                }
            }
            "-h" | "--help" => {
                println!("{HELP_MSG}");
                return Ok(());
            }
            arg => {
                return Err(anyhow!("Unknown argument \"{arg}\""));
            }
        }
    }

    // Initialise configuration
    let _ = RESOURCES_DIR.set(PathBuf::from(resources.unwrap_or("resources".into())));
    let _ = WORK_DIR.set(PathBuf::from(work_dir.unwrap_or("working".into())));
    let _ = ATOM_NAME_MAP.set(AtomNameMap::create());

    // Set up connection to server. Will retry if server is unavailable or connection fails.
    let running = Arc::new(AtomicBool::new(true));
    let _ = PytfWorker::connect(server_addr, key, running.clone()).await;

    // Gracefully handle Ctrl-C so worker doesn't try to reconnect when stopped
    ctrlc::set_handler(move || { running.store(false, Ordering::SeqCst); })?;

    let _ = actix_rt::signal::ctrl_c().await?;
    Ok(())
}


const HELP_MSG: &str = "
USAGE: pytf-worker <server_ip:port> <worker_password> [OPTIONS]

  OPTION            ARG         DESCRIPTION

  -r/--resources    <dir>       Resources directory to use. Defaults to ./resources

  -w/--work-dir     <dir>       Working directory in which to store PyThinFilm runs.
                                Defaults to ./working and will be created if it doesn't exist.

  -h/--help                     Show this message and exit.
";
