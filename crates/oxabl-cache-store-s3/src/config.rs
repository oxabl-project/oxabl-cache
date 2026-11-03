//! What an S3 store needs to know about its bucket.

use std::path::PathBuf;
use url::Url;

/// A bucket on an S3-compatible endpoint. Secrets are named by file, never held inline.
#[derive(Debug, Clone)]
pub struct S3Config {
    /// The service endpoint, `http` or `https`.
    pub endpoint: Url,
    /// The signing region.
    pub region: String,
    /// The bucket name.
    pub bucket: String,
    /// Key prefix inside the bucket; empty for the bucket root.
    pub prefix: String,
    /// Path-style addressing (`endpoint/bucket/key`) instead of virtual-host style.
    pub path_style: bool,
    /// File holding the access key id.
    pub access_key_id_file: PathBuf,
    /// File holding the secret access key.
    pub secret_access_key_file: PathBuf,
}
