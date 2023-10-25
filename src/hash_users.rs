use std::io::{BufWriter, Error, ErrorKind, Result, Write};

use argon2::{Argon2, password_hash::{SaltString, rand_core::OsRng}, PasswordHasher};

const HELP_MSG: &str =
"
USAGE: pytf-hash-users <in_file> [-o <out_file>] [-h]

  -o <out_file>     File to output to. Default is in-place (same as input)
  -h                Show this message.
";

fn main() -> Result<()>{
    let mut in_file: Option<String> = None;
    let mut out_file: Option<String> = None;
    let mut args = std::env::args().skip(1);
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
    let Some(in_file) = in_file else {
        return Err(
            Error::new(ErrorKind::InvalidData.into(),
            "Missing argument for users file")
        );
    };
    let out_file = match out_file {
        Some(fname) => fname,
        None => in_file.clone()
    };
    let in_data = std::fs::read_to_string(in_file)?;
    let mut out_data = BufWriter::new(
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(out_file)?
    );

    let argon2 = Argon2::default();
    let mut idx = 0;
    let mut users = 0;
    let mut rng = OsRng;
    for line in in_data.lines() {
        idx += 1;
        if line.len() == 0 { continue }
        let Some((username, password)) = line.split_once(",") else {
            return Err(Error::new(ErrorKind::InvalidData, format!("Missing ',' on line {idx}")));
        };
        let salt = SaltString::generate(&mut rng);
        let Ok(hash) = argon2.hash_password(password.as_bytes(), &salt) else {
            return Err(Error::new(ErrorKind::Other, format!("Error hashing password on line {idx}")))
        };
        writeln!(out_data, "{username},{hash}")?;
        users += 1;
    }
    out_data.flush()?;
    println!("Hashed passwords for {users} users.");
    Ok(())
}
