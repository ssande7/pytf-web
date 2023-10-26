use std::{io::{Error, ErrorKind}, path::PathBuf};

use pytf_web::{
    pytf_config::{AVAILABLE_MOLECULES, MoleculeResources, RESOURCES_DIR},
    authentication::{USER_DB, UserDB}
};

use crate::job_queue::ARCHIVE_DIR;


#[derive(Clone, Debug)]
pub struct Connection {
    pub address: String,
    pub port: u16,
}

/// Parse command line arguments for the server to set relevant globals
/// Returns connection details for the server and the redis server (in that order)
pub fn parse_args() -> anyhow::Result<Option<(Connection, Connection)>> {
    let mut args = std::env::args().skip(1).peekable();
    let mut resources = None;
    let mut archive_dir = None;
    let mut mols_file = None;
    let mut users_file = None;
    let mut address = Connection { address: "127.0.0.1".into(), port: 8080 };
    let mut redis_address = Connection { address: "127.0.0.1".into(), port: 6379 };
    while let Some(arg) = args.next() {
        match arg.as_ref() {
            "-m" | "--molecules" => {
                mols_file = args.next();
                if mols_file.is_none() {
                    Err(Error::new(ErrorKind::InvalidInput, "Missing argument for molecules .json file"))?;
                };
            }
            "-r" | "--resources" => {
                resources = args.next();
                if resources.is_none() {
                    Err(Error::new(ErrorKind::InvalidInput, "Missing argument for resources directory"))?;
                }
            }
            "-a" | "--archive" => {
                archive_dir = args.next();
                if archive_dir.is_none() {
                    Err(Error::new(ErrorKind::InvalidInput, "Missing argument for archive directory"))?;
                }
            }
            "-u" | "--users" => {
                users_file = args.next();
                if users_file.is_none() {
                    Err(Error::new(ErrorKind::InvalidInput, "Missing argument for users file"))?;
                };
            }
            "-ip" => {
                let Some(addr) = args.next() else {
                    Err(Error::new(ErrorKind::InvalidInput, "Missing argument for server ip address"))?;
                    unreachable!();
                };
                address.address = addr;
            }
            "-p" | "--port" => {
                let Some(port) = args.next() else {
                    Err(Error::new(ErrorKind::InvalidInput, "Missing argument for server port"))?;
                    unreachable!();
                };
                address.port = port.parse()?;
            }
            "--redis-ip" => {
                let Some(addr) = args.next() else {
                    Err(Error::new(ErrorKind::InvalidInput, "Missing argument for server ip address"))?;
                    unreachable!();
                };
                redis_address.address = addr;
            }
            "--redis-port" => {
                let Some(port) = args.next() else {
                    Err(Error::new(ErrorKind::InvalidInput, "Missing argument for server port"))?;
                    unreachable!();
                };
                redis_address.port = port.parse()?;
            }
            "-h" | "--help" => {
                println!("{HELP_MSG}");
                return Ok(None);
            }
            _ => {
                Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!("Unknown argument {arg}")
                ))?;
            }
        }
    }

    // Resources directory
    let _ = RESOURCES_DIR.set(PathBuf::from(resources.unwrap_or("resources".into())));
    let _ = ARCHIVE_DIR.set(PathBuf::from(archive_dir.unwrap_or("archive".into())));

    // Molecules
    let mols_file = match mols_file {
        Some(fname) => std::path::PathBuf::from(fname),
        None => std::path::PathBuf::from(RESOURCES_DIR.get().unwrap()).join("molecules.json"),
    };
    let _ = AVAILABLE_MOLECULES.set(MoleculeResources::load(mols_file)?);

    // Users
    let _ = USER_DB.set(match users_file {
        Some(fname) => UserDB::load(std::path::PathBuf::from(fname))?,
        None => {
            log::warn!("No user database provided.");
            UserDB::default()
        }
    });

    Ok(Some((address, redis_address)))
}

const HELP_MSG: &str = "
USAGE: pytf-server [OPTIONS]

  OPTION          ARG       DESCRIPTION

  -u/--users      <file>    File containing usernames and password hashes, one per line,
                            separated by a comma. Can be generated from plaintext .csv using
                            the included pytf-hash-users tool.

  -m/--molecules  <file>    JSON file containing the available molecules. See docs for details.
                            Defaults to {RESOURCES_DIR}/molecules.json

  -r/--resources  <dir>     Resources directory to use. Defaults to ./resources

  -a/--archive    <dir>     Archive directory to store old inactive jobs. Defaults to ./archive

  -ip             <IP>      IP address of server. Defaults to 127.0.0.1

  --port          <port>    Port for the server to listen on. Defaults to 8080

  --redis-ip      <IP>      IP address of the Redis server. Defaults to 127.0.0.1

  --redis-port    <port>    Port of the Redis server. Defaults to 6379

  -h/--help                 Show this message and exit.
";

