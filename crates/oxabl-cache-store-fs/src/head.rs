//! Size and ETag of a stored file.

use oxabl_cache_core::ports::ObjectHead;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// The head of a file: its size and an ETag built from size and modification time.
/// Objects are immutable, so a changed ETag means the file was replaced.
pub(crate) fn head_of(path: &Path) -> std::io::Result<ObjectHead> {
    let meta = fs::metadata(path)?;
    let nanos = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    Ok(ObjectHead {
        size: meta.len(),
        etag: format!("{nanos:x}-{:x}", meta.len()),
    })
}
