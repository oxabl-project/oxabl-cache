//! Size and ETag of a stored file.

use oxabl_cache_core::encoding::hex_encode;
use oxabl_cache_core::ports::ObjectHead;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::UNIX_EPOCH;

/// Files whose digest is remembered; the map is emptied when it fills.
const CACHE_ENTRIES: usize = 4096;

/// The ETag of a file's bytes: the first 128 bits of their SHA-256, in hex.
/// It depends on nothing but the content, so a replacement with other bytes
/// always changes it, whatever the clock, and it survives reads and restarts.
pub(crate) fn etag_of(digest: &[u8]) -> String {
    hex_encode(&digest[..16])
}

/// What `stat` says about a file; a digest is reused only while it is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    size: u64,
    modified_ns: u128,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl Fingerprint {
    fn of(file: &File) -> std::io::Result<Self> {
        let meta = file.metadata()?;
        Ok(Self {
            size: meta.len(),
            modified_ns: meta
                .modified()?
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos()),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
            },
        })
    }
}

/// Digests of files already read, so a range read does not hash the whole
/// object again. Shared by the clones of a store and its writers.
#[derive(Debug, Default)]
pub(crate) struct EtagCache(Mutex<HashMap<PathBuf, (Fingerprint, String)>>);

impl EtagCache {
    /// Forgets `path`: its file was replaced or removed.
    pub(crate) fn forget(&self, path: &Path) {
        self.entries().remove(path);
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, (Fingerprint, String)>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Opens `path` and returns the file with its head. The head describes the file
/// that was opened, even if the name is replaced afterwards.
pub(crate) fn open_with_head(
    cache: &EtagCache,
    path: &Path,
) -> std::io::Result<(File, ObjectHead)> {
    let mut file = File::open(path)?;
    let before = Fingerprint::of(&file)?;
    let cached = cache
        .entries()
        .get(path)
        .filter(|(seen, _)| *seen == before)
        .map(|(_, etag)| etag.clone());
    let etag = match cached {
        Some(etag) => etag,
        None => {
            let etag = etag_of(&digest_of(&mut file)?);
            // A file that changed while it was read is not remembered.
            if Fingerprint::of(&file)? == before {
                let mut entries = cache.entries();
                if entries.len() >= CACHE_ENTRIES {
                    entries.clear();
                }
                entries.insert(path.to_owned(), (before, etag.clone()));
            }
            etag
        }
    };
    Ok((
        file,
        ObjectHead {
            size: before.size,
            etag,
        },
    ))
}

/// The head of the file at `path`.
pub(crate) fn head_of(cache: &EtagCache, path: &Path) -> std::io::Result<ObjectHead> {
    open_with_head(cache, path).map(|(_, head)| head)
}

fn digest_of(file: &mut File) -> std::io::Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(hasher.finalize().into());
        }
        hasher.update(&buffer[..read]);
    }
}
