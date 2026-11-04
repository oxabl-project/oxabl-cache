use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use oxabl_cache_crypto::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

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

#[derive(Serialize)]
struct Written<'a> {
    token: &'a str,
    expires_at: u64,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "session file")
}

impl StoredSession {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    fn read(&self) -> io::Result<Option<Stored>> {
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
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES).read_to_end(&mut bytes)?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| invalid())
    }

    /// The token of an unexpired session. A file readable by others, or one that cannot be
    /// parsed, is an error; a missing or expired one is `None`.
    pub fn load(&self) -> io::Result<Option<SecretString>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Ok(self
            .read()?
            .filter(|stored| stored.expires_at > now)
            .map(|stored| SecretString::from(stored.token)))
    }

    /// The token of the stored session, expired or not.
    pub fn load_any(&self) -> io::Result<Option<SecretString>> {
        Ok(self.read()?.map(|stored| SecretString::from(stored.token)))
    }

    /// Replaces the stored session atomically, with mode 0600 in a directory of mode 0700.
    pub fn save(&self, token: &SecretString, expires_at: u64) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no state directory",
            ));
        };
        let dir = path.parent().ok_or_else(invalid)?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let staging = dir.join(format!(
            ".{}.tmp",
            path.file_name().ok_or_else(invalid)?.to_string_lossy()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&staging)?;
        let written = Written {
            token: token.expose_secret(),
            expires_at,
        };
        file.write_all(&serde_json::to_vec(&written).map_err(|_| invalid())?)?;
        file.sync_all()?;
        fs::rename(&staging, path)?;
        File::open(dir)?.sync_all()
    }

    /// Deletes the stored session; whether there was one.
    pub fn remove(&self) -> io::Result<bool> {
        let Some(path) = &self.path else {
            return Ok(false);
        };
        match fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}
