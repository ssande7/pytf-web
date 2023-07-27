use actix_cors::Cors;
use actix_web::{HttpServer, http, App, post, web, Responder, HttpResponse};
use std::{io::prelude::*, path::PathBuf};

use pytf_web::pytf_config::*;
use pytf_web::pytf_runner::*;

// PLAN: Merge into single executable with --mode=server or --mode=worker options.
//       - Server started with a list of worker addresses
//       - Server receives request to run simulation with minimal PytfConfig json object
//       - Server calls `canonicalize_ratios()` and sets `(config.name = Some(config.get_name())`
//       - Server chooses worker to handle the config:
//          * If a worker is currently running the specified config, just link the user to that
//            worker.
//          * If a worker previously handled the specified config and completed it, just link the
//            user to that worker.
//          * If a worker previously handled the specified config but didn't complete it and is now
//            working on something else, send the config to a different worker with instruction to
//            copy previous work from the previous worker. This new worker is now the owner of that
//            config.
//      - When a worker receives a new config to work on, it calls `config.prefill()` to fill out
//        the remaining details, then creates a config.yml file and sets up a pytf deposition
//        object to work on it.
//      - Users connected directly to the relevant worker via WebSockets. Worker streams trajectory
//        (in CBOR format?) to user as each pack of frames (one .xtc file) becomes available

// TODO: Make this require authentication governed by a shared key between server and workers
#[post("/deposit")]
async fn run_deposition(mut config: web::Json<PytfConfig>, pytf_handle: web::Data<PytfHandle>) -> impl Responder {
    // Fill extra fields for pytf compatibility and calculate unique name
    config.prefill();
    let jobname = config.name().unwrap(); // Safe to unwrap since name is set in prefill

    // Get yaml string to append to config
    let Ok(yml) = serde_yaml::to_string(&config) else {
        return HttpResponse::BadRequest().body(
            format!("Failed to convert config to yaml")
        )
    };

    // Create working directory if it doesn't already exist
    let mut config_yml = PathBuf::from(config.workdir().unwrap());
    if !config_yml.is_dir() {
        if let Err(e) = std::fs::create_dir(&config_yml) {
            return HttpResponse::BadRequest().body(
                format!("Failed to create job directory for {jobname}: {e}")
            )
        }
    }
    // Create config.yml in working directory if it doesn't already exist
    config_yml.push("config.yml");
    if !config_yml.is_file() {
        if let Err(e) = std::fs::copy("resources/base_config.yml", &config_yml) {
            return HttpResponse::InternalServerError().body(
                format!("Failed to copy base config file for {jobname}: {e}")
            )
        }
    
        // Write config.yml to jobname directory
        let mut config_file = match std::fs::OpenOptions::new()
            .write(true)
            .append(true)
            .open(&config_yml)
        {
            Ok(f) => f,
            Err(e) => {
                return HttpResponse::InternalServerError().body(
                    format!("Error opening {config_yml:?} for appending: {e}")
                )
            }
        };
        if let Err(e) = writeln!(config_file, "\n{}", yml) {
            return HttpResponse::InternalServerError().body(
                format!("Error writing config file for {jobname}: {e}")
            )
        }
    }

    // Start new pytf simulation
    pytf_handle.new_config(Some(config_yml));

    HttpResponse::Ok().finish()
}

// TODO: Make this require authentication governed by a shared key between server and workers
#[post("/stop")]
async fn stop_current_deposition(pytf_handle: web::Data<PytfHandle>) -> impl Responder {
    // Null out next_config so a new simulation isn't started on the next loop iteration
    pytf_handle.new_config(None);
    pytf_handle.stop();
    HttpResponse::Ok()
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {

    let runner = PytfRunner::new();
    let runner_handle = web::Data::new(runner.get_handle());
    let _ = std::thread::spawn(|| runner.start()); // Detach from pytf thread

    let address = "127.0.0.1";
    let port = 8081;

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
            .app_data(web::Data::clone(&runner_handle))
            .service(run_deposition)
            .wrap(cors)
    })
    .bind((address, port))?
    .run()
    .await
}

