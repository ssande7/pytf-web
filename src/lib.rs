pub mod authentication;
pub mod pytf;
pub mod pytf_config;
pub mod pytf_runner;
pub mod pytf_frame;
pub mod worker_client;

use anyhow::anyhow;

/// Split off a null-terminated utf8 string form a byte array, ignoring the null terminator
// TODO: Return a Result for better error reporting?
pub fn split_nullterm_utf8_str(bytes: &mut actix_web::web::Bytes) -> anyhow::Result<String> {
    let Some(nullterm) = bytes.iter().position(|&b| b == '\0' as u8)
    else { return Err(anyhow!("Failed to find null terminator")) };
    let substr = String::from_utf8(bytes.split_to(nullterm).into())?;
    let _ = bytes.split_to(1);
    Ok(substr)
}
