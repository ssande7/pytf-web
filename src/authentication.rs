use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env::Args,
    fs,
    io::{self, BufRead},
    sync::OnceLock,
};

pub static USER_DB: OnceLock<UserDB> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginToken {
    token: String,
}
impl From<String> for LoginToken {
    fn from(token: String) -> Self {
        Self { token }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserCredentials {
    pub username: String,
    pub password: String,
}

impl UserCredentials {
    pub fn get_id(&self) -> String {
        self.username.clone()
    }

    fn password_hash(&self) -> String {
        let argon2 = Argon2::default();
        let salt = SaltString::generate(&mut OsRng);
        argon2
            .hash_password(self.password.as_str().as_bytes(), &salt)
            .expect("Error hashing password")
            .to_string()
    }
}

#[derive(Debug, Default)]
pub struct UserDB(HashMap<String, String>);

impl UserDB {
    pub fn from_cli_or_default(mut args: Args) -> Self {
        while let Some(arg) = args.next() {
            if arg == "--users" || arg == "-u" {
                let Some(fname) = args.next() else {
                    log::warn!("no file specified with --users flag. Users will not be loaded!");
                    return Default::default()
                };
                log::debug!("Reading users from {fname}");

                let file = match fs::File::open(fname) {
                    Ok(fid) => fid,
                    Err(e) => {
                        log::error!("Error while opening users file: {e}");
                        return Default::default();
                    }
                };
                match UserDB::from_csv(file) {
                    Ok(db) => {
                        return db;
                    }
                    Err(e) => {
                        log::error!("error while reading users: {e}");
                        return Default::default();
                    }
                }
            }
        }
        Default::default()
    }

    /// Read a comma separated list
    pub fn from_csv(file: fs::File) -> io::Result<Self> {
        let mut fid = io::BufReader::new(file);
        let mut line = "".to_string();
        let mut idx = 0;
        let mut db = Self::default();
        while {
            line.clear();
            idx += 1;
            fid.read_line(&mut line)? > 0
        } {
            // line contains trailing \n, so ignore that.
            let Some((username, password)) = line[..(line.len()-1)].split_once(",") else {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Missing ',' on line {idx}")));
            };
            db.create_user(UserCredentials {
                username: username.into(),
                password: password.into(),
            });
        }
        Ok(db)
    }

    pub fn create_user(&mut self, user: UserCredentials) {
        let hash = user.password_hash();
        if self.0.insert(user.username.clone(), hash).is_some() {
            log::warn!("User already exists! ({})", user.username);
        }
    }

    pub fn validate_user(&self, user: &UserCredentials) -> bool {
        let Some(hash) = self.0.get(&user.username) else {
            log::info!("Received login request for unknown user \"{}\"", user.username);
            return false
        };
        let parsed_hash = match PasswordHash::new(&hash) {
            Ok(h) => h,
            Err(e) => {
                log::warn!("Failed to hash password for user \"{}\": {e}", user.username);
                return false
            }
        };
        Argon2::default()
            .verify_password(user.password.as_str().as_bytes(), &parsed_hash)
            .is_ok()
    }
}
