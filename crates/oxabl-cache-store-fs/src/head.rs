//! Size and ETag of a stored file.

use crate::stamp::{Stamp, keep_digest, kept_digest};
use oxabl_cache_core::encoding::hex_encode;
use oxabl_cache_core::ports::ObjectHead;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// The ETag of bytes with this SHA-256: its first 128 bits, in hex. It depends on
/// nothing but the content, so a replacement with other bytes always changes it,
/// whatever the clock, and it survives reads, restarts and copies of the store.
pub(crate) fn etag_of(digest: &[u8; 32]) -> String {
    hex_encode(&digest[..16])
}

/// Opens `path` and returns the file with its head. The head describes the file
/// that was opened, even if the name is replaced afterwards. A file without a kept
/// digest is hashed once, and the digest kept on it.
pub(crate) fn open_with_head(path: &Path) -> std::io::Result<(File, ObjectHead)> {
    let mut file = File::open(path)?;
    let stamp = Stamp::of(&file)?;
    let digest = match kept_digest(&file, stamp) {
        Some(digest) => digest,
        None => {
            let digest = digest_of(&mut file)?;
            if Stamp::of(&file)? == stamp {
                let _ = keep_digest(&file, stamp, &digest);
            }
            digest
        }
    };
    Ok((
        file,
        ObjectHead {
            size: stamp.size,
            etag: etag_of(&digest),
        },
    ))
}

/// The head of the file at `path`.
pub(crate) fn head_of(path: &Path) -> std::io::Result<ObjectHead> {
    open_with_head(path).map(|(_, head)| head)
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
