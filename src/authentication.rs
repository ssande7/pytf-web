use argon2::{
    password_hash::{PasswordHash, PasswordVerifier},
    Argon2,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{self, BufRead},
    sync::OnceLock,
    path::Path,
};

/// Database of usernames and associated password hashes
pub static USER_DB: OnceLock<UserDB> = OnceLock::new();

/// Login token (username) to send back to client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginToken {
    token: String,
}
impl From<String> for LoginToken {
    fn from(token: String) -> Self {
        Self { token }
    }
}

/// Login credentials to be validated
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserCredentials {
    pub username: String,
    pub password: String,
}

impl UserCredentials {
    pub fn get_id(&self) -> String {
        self.username.clone()
    }
}

/// Database of usernames and associated password hashes.
#[derive(Debug, Default)]
pub struct UserDB(HashMap<String, String>);

impl UserDB {
    /// Load the users database from the specified file.
    /// File should contain lines of username,argon2_password_hash (no space between!)
    pub fn load(fname: impl AsRef<Path>) -> std::io::Result<Self> {
        log::debug!("Reading users from {}", fname.as_ref().to_string_lossy());
        let out = UserDB::from_csv(fs::File::open(fname)?)?;
        log::debug!("Done reading users");
        Ok(out)
    }

    /// Read a comma separated list of username,argon2_password_hash (no space between!)
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
            let Some((username, hash)) = line.trim_end_matches('\n').split_once(",") else {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Missing ',' on line {idx}")));
            };
            if let Err(e) = PasswordHash::new(&hash) {
                log::error!("Invalid password hash for user \"{username}\". Failed to parse with error: {e}");
                continue
            }
            if db.0.insert(username.into(), hash.into()).is_some() {
                log::warn!("User already exists! ({})", username);
            }
        }
        Ok(db)
    }

    /// Check the provided credentials against the database and return true if a match is found
    /// and the password is correct.
    pub fn validate_user(&self, user: &UserCredentials) -> bool {
        let Some(hash) = self.0.get(&user.username) else {
            log::info!("Received login request for unknown user \"{}\"", user.username);
            return false
        };
        let parsed_hash = match PasswordHash::new(&hash) {
            Ok(h) => h,
            Err(e) => {
                log::warn!("Failed to parse password hash for user \"{}\". This should never happen! Error was: {e}", user.username);
                return false
            }
        };
        Argon2::default()
            .verify_password(user.password.as_str().as_bytes(), &parsed_hash)
            .is_ok()
    }
}
