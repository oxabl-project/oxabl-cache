//! Where objects live in the bucket.

use oxabl_cache_core::Error;
use oxabl_cache_core::ids::{CacheId, PackId};
use oxabl_cache_core::ports::ObjectKey;

/// Key construction under an optional prefix.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    prefix: String,
}

fn plain_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

impl Layout {
    /// Accepts a prefix of plain path segments; surrounding slashes are dropped.
    pub(crate) fn new(prefix: &str) -> Result<Self, Error> {
        let trimmed = prefix.trim_matches('/');
        if !trimmed.is_empty() && !trimmed.split('/').all(plain_segment) {
            return Err(Error::bad_request(
                "store.prefix is slash-separated names of letters, digits, `-`, `_` and `.`",
            ));
        }
        let prefix = if trimmed.is_empty() {
            String::new()
        } else {
            format!("{trimmed}/")
        };
        Ok(Self { prefix })
    }

    /// `<prefix>v1/<cache>/<pack>`.
    pub(crate) fn object(&self, key: &ObjectKey) -> String {
        format!("{}{}", self.cache_prefix(&key.cache), key.pack)
    }

    /// `<prefix>v1/<cache>/`.
    pub(crate) fn cache_prefix(&self, cache: &CacheId) -> String {
        format!("{}v1/{cache}/", self.prefix)
    }

    /// `<prefix>probe/<run>/`, outside every cache prefix.
    pub(crate) fn probe_prefix(&self, run: &str) -> String {
        format!("{}probe/{run}/", self.prefix)
    }

    /// The pack a listed key names under `cache_prefix`, if it is exactly one pack id.
    pub(crate) fn pack_of(cache_prefix: &str, key: &str) -> Option<PackId> {
        key.strip_prefix(cache_prefix)?.parse().ok()
    }
}
