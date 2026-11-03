use std::collections::HashSet;

use oxabl_cache_format::limits::{FILES_PER_SET, SET_PATH_BYTES};

use super::types::{ArtifactFileRef, StoreFailure};

/// Checks what `put` can check without touching the disk: file count, unique paths, path
/// bytes and the plaintext cap.
pub(crate) fn validate(files: &[ArtifactFileRef], max_plain: u64) -> Result<u64, StoreFailure> {
    if files.is_empty() || files.len() > FILES_PER_SET {
        return Err(StoreFailure::Rejected);
    }
    let mut seen = HashSet::new();
    let mut path_bytes = 0usize;
    let mut total = 0u64;
    for file in files {
        if !seen.insert(file.path.as_str()) {
            return Err(StoreFailure::Rejected);
        }
        path_bytes += file.path.as_str().len();
        total = total.checked_add(file.size).ok_or(StoreFailure::TooLarge)?;
    }
    if path_bytes > SET_PATH_BYTES {
        return Err(StoreFailure::Rejected);
    }
    if total > max_plain {
        return Err(StoreFailure::TooLarge);
    }
    Ok(total)
}
