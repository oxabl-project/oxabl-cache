//! The digest of an object's bytes, kept in an extended attribute of its file.

use std::fs::File;
use std::io;
use std::time::UNIX_EPOCH;

/// What `stat` says about a file's bytes; a kept digest holds only while it is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub(crate) size: u64,
    modified_ns: u128,
}

impl Stamp {
    pub(crate) fn of(file: &File) -> io::Result<Self> {
        let meta = file.metadata()?;
        Ok(Self {
            size: meta.len(),
            modified_ns: meta
                .modified()?
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos()),
        })
    }

    fn encode(self, digest: &[u8; 32]) -> [u8; 56] {
        let mut value = [0u8; 56];
        value[..32].copy_from_slice(digest);
        value[32..40].copy_from_slice(&self.size.to_be_bytes());
        value[40..].copy_from_slice(&self.modified_ns.to_be_bytes());
        value
    }
}

#[cfg(target_os = "linux")]
const NAME: &str = "user.oxabl-cache.sha256";

/// The digest kept on `file`, if one was kept while its bytes were as `stamp` says.
#[cfg(target_os = "linux")]
pub(crate) fn kept_digest(file: &File, stamp: Stamp) -> Option<[u8; 32]> {
    let mut value = [0u8; 56];
    let read = rustix::fs::fgetxattr(file, NAME, &mut value).ok()?;
    let digest: [u8; 32] = value[..32].try_into().ok()?;
    (read == value.len() && stamp.encode(&digest) == value).then_some(digest)
}

/// Keeps `digest` on `file`, whose bytes are as `stamp` says. A file system without
/// extended attributes keeps nothing, and every read of the file hashes it.
#[cfg(target_os = "linux")]
pub(crate) fn keep_digest(file: &File, stamp: Stamp, digest: &[u8; 32]) -> io::Result<()> {
    use rustix::io::Errno;
    match rustix::fs::fsetxattr(
        file,
        NAME,
        &stamp.encode(digest),
        rustix::fs::XattrFlags::empty(),
    ) {
        Ok(()) | Err(Errno::NOTSUP) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn kept_digest(_file: &File, _stamp: Stamp) -> Option<[u8; 32]> {
    None
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn keep_digest(_file: &File, _stamp: Stamp, _digest: &[u8; 32]) -> io::Result<()> {
    Ok(())
}
