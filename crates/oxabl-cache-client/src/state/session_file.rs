use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use oxabl_cache_crypto::SecretString;
use serde::Deserialize;

const MAX_BYTES: u64 = 16 * 1024;

/// The login session the member CLI stores for one tenant.
pub(crate) struct StoredSession {
    path: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Stored {
    token: String,
    expires_at: u64,
}

impl StoredSession {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    /// The token of an unexpired session. A file readable by others, or one that cannot be
    /// parsed, is an error; a missing or expired one is `None`.
    pub fn load(&self) -> io::Result<Option<SecretString>> {
        let Some(path) = &self.path else {
            return Ok(None);
        };
        let file = match File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let meta = file.metadata()?;
        if !meta.is_file() || meta.len() > MAX_BYTES || meta.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "session file"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES).read_to_end(&mut bytes)?;
        let stored: Stored = serde_json::from_slice(&bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "session file"))?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Ok((stored.expires_at > now).then(|| SecretString::from(stored.token)))
    }
}
