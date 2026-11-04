use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use nix::fcntl::OFlag;

use crate::artifact_path::ArtifactPath;
use crate::compile_key::CompileKey;

/// A verified output as the caller saw it: stored under `path`, read from `disk_path`.
#[derive(Debug, Clone)]
pub struct ArtifactFileRef {
    pub path: ArtifactPath,
    /// Absolute; read once, lazily. Only the final component must not be a symlink.
    pub disk_path: PathBuf,
    pub size: u64,
    pub blake3: [u8; 32],
}

impl ArtifactFileRef {
    /// Measures the file at `disk_path` and refers to it as `path`. The final component must
    /// not be a symlink and the file must be a regular one.
    pub fn from_disk(path: ArtifactPath, disk_path: PathBuf) -> io::Result<Self> {
        let disk_path = std::path::absolute(disk_path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags((OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK).bits())
            .open(&disk_path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file",
            ));
        }
        let mut hasher = blake3::Hasher::new();
        let mut size = 0u64;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            size += read as u64;
        }
        Ok(Self {
            path,
            disk_path,
            size,
            blake3: *hasher.finalize().as_bytes(),
        })
    }
}

/// Why a set was not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreFailure {
    /// The set breaks a rule: duplicate paths, too many files or path bytes, or the server
    /// refused it.
    Rejected,
    /// The set or its sealed segment exceeds a size limit.
    TooLarge,
    /// A file is missing, not a regular file, a symlink in its final component, or not readable.
    Unreadable,
    QuotaExceeded,
    /// The server or the object store could not be reached or answered an error.
    Unavailable,
}

/// What became of one set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOutcome {
    Committed,
    /// Another write won; the cache keeps that one.
    Existed,
    /// A file's size or BLAKE3 differed from its `ArtifactFileRef` when read; nothing was
    /// uploaded for the key.
    Changed,
    Failed(StoreFailure),
}

/// The outcome of every set an uploader was fed, in the order of `put`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreReport {
    pub outcomes: Vec<(CompileKey, StoreOutcome)>,
    pub bytes_uploaded: u64,
}
