//! Credentials read from files named in the configuration.

use oxabl_cache_core::{Error, Result};
use rusty_s3::Credentials;
use std::fs;
use std::path::Path;

fn read(path: &Path, what: &str) -> Result<String> {
    let text = fs::read_to_string(path).map_err(|error| {
        Error::bad_request(format!(
            "cannot read {what} file {}: {error}",
            path.display()
        ))
    })?;
    let value = text.trim();
    if value.is_empty() {
        return Err(Error::bad_request(format!(
            "{what} file {} is empty",
            path.display()
        )));
    }
    Ok(value.to_owned())
}

/// Reads the key id and the secret, each trimmed of surrounding whitespace.
pub(crate) fn from_files(key_id: &Path, secret: &Path) -> Result<Credentials> {
    Ok(Credentials::new(
        read(key_id, "access key id")?,
        read(secret, "secret access key")?,
    ))
}
