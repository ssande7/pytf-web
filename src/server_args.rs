use std::io::{Error, ErrorKind};

use pytf_web::{
    pytf_config::{AVAILABLE_MOLECULES, MoleculeResources, RESOURCES_DIR},
    authentication::{USER_DB, UserDB}
};


#[derive(Clone, Debug)]
pub struct Connection {
    pub address: String,
    pub port: u16,
}

/// Parse command line arguments for the server to set relevant globals
/// Returns connection details for the server and the redis server (in that order)
pub fn parse_args() -> anyhow::Result<(Connection, Connection)> {
    let mut args = std::env::args().skip(1).peekable();
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
            _ => {
                Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!("Unknown argument {arg}")
                ))?;
            }
        }
    }

    // Molecules
    let mols_file = match mols_file {
        Some(fname) => std::path::PathBuf::from(fname),
        None => {
            let mut path = std::path::PathBuf::from(format!("{RESOURCES_DIR}"));
            path.push("name_map.json");
            path
        }
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

    Ok((address, redis_address))
}

