use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;

use nix::fcntl::OFlag;

use super::types::{ArtifactFileRef, StoreFailure, StoreOutcome};

/// Why a file's bytes could not be taken.
pub(crate) enum ReadFailure {
    /// Not readable as a regular file.
    Unreadable,
    /// Its size or BLAKE3 differs from what the caller verified.
    Changed,
}

impl ReadFailure {
    pub fn outcome(self) -> StoreOutcome {
        match self {
            Self::Unreadable => StoreOutcome::Failed(StoreFailure::Unreadable),
            Self::Changed => StoreOutcome::Changed,
        }
    }
}

/// Reads a file once, into memory, and checks its size and BLAKE3 against the reference.
/// The final path component must not be a symlink and the file must be a regular one.
pub(crate) fn read_verified(file: &ArtifactFileRef) -> Result<Vec<u8>, ReadFailure> {
    if !file.disk_path.is_absolute() {
        return Err(ReadFailure::Unreadable);
    }
    let handle = OpenOptions::new()
        .read(true)
        .custom_flags((OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK).bits())
        .open(&file.disk_path)
        .map_err(|_| ReadFailure::Unreadable)?;
    let meta = handle.metadata().map_err(|_| ReadFailure::Unreadable)?;
    if !meta.is_file() {
        return Err(ReadFailure::Unreadable);
    }
    let mut bytes =
        Vec::with_capacity(usize::try_from(file.size).map_err(|_| ReadFailure::Changed)?);
    handle
        .take(file.size + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ReadFailure::Unreadable)?;
    if bytes.len() as u64 != file.size || blake3::hash(&bytes).as_bytes() != &file.blake3 {
        return Err(ReadFailure::Changed);
    }
    Ok(bytes)
}
