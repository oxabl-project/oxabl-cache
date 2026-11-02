//! Where objects live under the store root.

use oxabl_cache_core::ids::{CacheId, PackId};
use oxabl_cache_core::ports::ObjectKey;
use std::path::{Path, PathBuf};

/// `<root>/v1/<cache>/<pack[0..2]>/<pack>`.
pub(crate) fn object_path(root: &Path, key: &ObjectKey) -> PathBuf {
    let pack = key.pack.to_string();
    cache_dir(root, &key.cache).join(&pack[..2]).join(pack)
}

/// `<root>/v1/<cache>`.
pub(crate) fn cache_dir(root: &Path, cache: &CacheId) -> PathBuf {
    root.join("v1").join(cache.to_string())
}

/// `<root>/tmp`, beside `v1` so renames stay on one filesystem.
pub(crate) fn tmp_dir(root: &Path) -> PathBuf {
    root.join("tmp")
}

/// The pack a file name denotes, if it is a canonical pack id.
pub(crate) fn pack_of_name(name: &str) -> Option<PackId> {
    name.parse().ok()
}
