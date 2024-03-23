use std::io::{BufWriter, Error, ErrorKind, Result, Write};

use argon2::{Argon2, password_hash::{SaltString, rand_core::OsRng}, PasswordHasher};

const HELP_MSG: &str =
"
USAGE: pytf-hash-users <file> [OPTIONS]

OPTIONS:
  -g/--generate     <num_students> <pwd>
                                Generate the users file with the specified number of
                                students, all with the password <pwd>. A \"worker\"
                                user is also generated with a randomised password,
                                which is printed as output.

  -w/--worker-pass  <pwd>       Specify password for \"worker\" user when the -g flag
                                is passed. Default is to generate a password and print it.

  -o                <out_file>  File to output to. Default is in-place (same as <file>)

  -u/--user         <username> <pwd>
                                Additional user/password combinations to add.
                                Can be specified multiple times.

  -h                            Show this message.
";

fn main() -> Result<()>{
    let mut in_file: Option<String> = None;
    let mut out_file: Option<String> = None;
    let mut args = std::env::args().skip(1);
    let mut generate = 0usize;
    let mut student_pass: Option<String> = None;
    let mut worker_pass: Option<String> = None;
    let mut other_users: Vec<(String, String)> = vec![];
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP_MSG}");
                return Ok(())
            }
            "-o" | "--output" => {
                if out_file.is_some() {
                    eprintln!("WARNING: Multiple -o flags. Only the last value will be used.");
                }
                out_file = args.next();
                if out_file.is_none() {
                    return Err(
                        Error::new(ErrorKind::NotFound.into(),
                        "ERROR: No output file provided with -o")
                    );
                }
            },
            "-g" | "--generate" => {
                let Some(num_students) = args.next() else {
                    return Err(
                        Error::new(ErrorKind::InvalidInput.into(),
                        "ERROR: Missing argument with -g flag for number of student users to generate")
                    );
                };
                generate = match num_students.parse() {
                    Ok(n) => n,
                    Err(e) => {
                        return Err(
                            Error::new(ErrorKind::InvalidInput.into(),
                            format!("Error parsing number of students to generate: {e}"))
                        );
                    }
                };
                student_pass = args.next();
                if student_pass.is_none() {
                    return Err(
                        Error::new(ErrorKind::InvalidInput.into(),
                        "ERROR: No password specified for generated student users")
                    );
                }
            }
            "-w" | "--worker-pass" => {
                worker_pass = args.next();
                if worker_pass.is_none() {
                    return Err(
                        Error::new(ErrorKind::InvalidInput.into(),
                        "ERROR: No password specified with -w flag")
                    );
                }
            }
            "-u" | "--user" => {
                let username = args.next();
                let pass = args.next();
                match (username, pass) {
                    (Some(username), Some(pass)) => {
                        other_users.push((username, pass));
                    }
                    _ => return Err(
                        Error::new(ErrorKind::InvalidInput.into(),
                        "ERROR: Missing username or password after -u/--user")
                    )
                };
            }
            arg => {
                if in_file.is_some() {
                    return Err(
                        Error::new(ErrorKind::InvalidData.into(),
                        format!("ERROR: Unknown argument `{arg}`"))
                    );
                }
                in_file = Some(arg.into());
            }
        }
    }

    let out_file = match out_file {
        Some(fname) => fname,
        None => match &in_file {
            Some(fname) => fname.clone(),
            None => return Err(
                Error::new(ErrorKind::InvalidData.into(),
                "Missing argument for output users file")
            ),
        }
    };

    let mut hasher = HashWriter::new(
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(out_file)?
    );
    if generate > 0 {
        let target_len = format!("{generate}").len();
        hasher.hash_from_stream(
            (1..=generate).map(|i| {
                let num = format!("{i}");
                let prefix: String = vec!['0'; target_len - num.len()].iter().collect();
                (format!("student{}{}", prefix, num), student_pass.as_ref().unwrap())
            })
        )?;
        if let Some(worker_pass) = worker_pass {
            hasher.write_user("worker", &worker_pass)?;
        } else {
            let worker_key = SaltString::generate(OsRng);
            hasher.write_user("worker", &worker_key)?;
            println!("{worker_key}");
        }
        println!("Generated {generate} users.");
    } else if let Some(in_file) = in_file {
        let mut idx = 0;
        let in_data = std::fs::read_to_string(&in_file)?;
        hasher.hash_from_stream(
            in_data.lines().filter_map(|line| {
                idx += 1;
                if line.len() == 0 { return None }
                let Some(data) = line.split_once(",") else {
                    eprintln!("WARNING: Missing ',' on line {idx}! User skipped.");
                    return None;
                };
                Some(data)
            })
        )?;
        println!("Hashed passwords for {idx} users from {in_file}.");
    };
    if other_users.len() > 0 {
        for (user, pass) in &other_users {
            hasher.write_user(&user, &pass)?;
        }
        println!("Created {} additional users.", other_users.len());
    }
    hasher.flush()?;
    Ok(())
}

struct HashWriter<'a, W: Write> {
    argon2: Argon2<'a>,
    fid: BufWriter<W>,
    rng: OsRng,
}

impl<'a, W: Write> HashWriter<'a, W> {
    /// Create a HashWriter which writes to the specified file (via a `BufWriter`).
    fn new(file: W) -> Self {
        Self {
            argon2: Argon2::default(),
            fid: BufWriter::new(file),
            rng: OsRng,
        }
    }

    /// Write the specified `username` and `password` combination (password is hashed)
    fn write_user(&mut self, username: impl AsRef<str>, password: impl AsRef<str>) -> Result<()> {
        let username = username.as_ref();
        let password = password.as_ref();
        let salt = SaltString::generate(&mut self.rng);
        let Ok(hash) = self.argon2.hash_password(password.as_bytes(), &salt) else {
            return Err(Error::new(ErrorKind::Other, format!("Error hashing \"{password}\"")))
        };
        writeln!(self.fid, "{username},{hash}")
    }

    /// Write all (username, password) combinations from the provided stream. Passwords are hashed.
    fn hash_from_stream(&mut self, stream: impl Iterator<Item=(impl AsRef<str>, impl AsRef<str>)>) -> Result<()> {
        for (username, password) in stream {
            self.write_user(username, password)?;
        }
        Ok(())
    }

    /// Flush the internal `BufWriter`
    fn flush(&mut self) -> Result<()> {
        self.fid.flush()
    }
}
