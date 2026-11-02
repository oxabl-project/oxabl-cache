//! Secrets read from files named in the configuration, never from its text.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

fn decode(text: &str) -> Option<[u8; 32]> {
    oxabl_cache_core::encoding::hex_decode(text)
}

/// Reads one or two 32-byte keys, each as 64 lowercase hex characters on its own
/// line. The first signs object URLs; all verify.
pub(crate) fn read_url_keys(path: &Path) -> Result<Vec<[u8; 32]>, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read key file {}: {error}", path.display()))?;
    let keys: Vec<[u8; 32]> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            decode(line)
                .ok_or_else(|| format!("{}: a key is 64 lowercase hex characters", path.display()))
        })
        .collect::<Result<_, _>>()?;
    if keys.is_empty() || keys.len() > 2 {
        return Err(format!("{}: expected one or two keys", path.display()));
    }
    Ok(keys)
}

/// Warns through `warn` when a secret file can be read by other users.
pub(crate) fn check_permissions(path: &Path, warn: impl FnOnce(String)) {
    if let Ok(meta) = fs::metadata(path)
        && meta.permissions().mode() & 0o077 != 0
    {
        warn(format!(
            "{} is readable by other users; chmod 600 it",
            path.display()
        ));
    }
}

/// Creates a key file with a fresh random key if it does not exist; returns whether it did.
pub(crate) fn create_if_missing(path: &Path) -> Result<bool, String> {
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).map_err(|error| format!("randomness: {error}"))?;
    let text = format!("{}\n", oxabl_cache_core::encoding::hex_encode(&key));
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(text.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(format!("cannot create {}: {error}", path.display())),
    }
}
