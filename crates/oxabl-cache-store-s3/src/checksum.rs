//! The `x-amz-checksum-sha256` header value.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};

/// Base64 of a raw SHA-256 digest.
pub(crate) fn header_value(digest: &[u8; 32]) -> String {
    STANDARD.encode(digest)
}

/// The digest of `bytes`.
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
