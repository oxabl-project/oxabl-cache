//! HMAC-signed object URLs in the shape of presigned ones.

use crate::base64url;
use hmac::{Hmac, KeyInit, Mac};
use oxabl_cache_core::ids::{CacheId, PackId};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Why a signed URL was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlRejection {
    /// The expiry has passed.
    Expired,
    /// The signature does not match under any configured key.
    BadSignature,
}

/// The URL path of a pack object.
pub fn object_path(cache: &CacheId, pack: &PackId) -> String {
    format!("/v1/caches/{cache}/objects/{pack}")
}

fn message(method: &str, path: &str, expires_at: i64, content_length: Option<u64>) -> String {
    let length = content_length.map(|n| n.to_string()).unwrap_or_default();
    format!("{method}\n{path}\n{expires_at}\n{length}")
}

fn mac(key: &[u8], text: &str) -> HmacSha256 {
    let mut mac =
        <HmacSha256 as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(text.as_bytes());
    mac
}

/// Signs `method path exp content-length` with `key`; the signature is base64url.
pub(crate) fn sign(
    key: &[u8],
    method: &str,
    path: &str,
    expires_at: i64,
    content_length: Option<u64>,
) -> String {
    let tag = mac(key, &message(method, path, expires_at, content_length)).finalize();
    base64url::encode(&tag.into_bytes())
}

/// Checks a signature against every key, in constant time per key.
pub(crate) fn verify(
    keys: &[[u8; 32]],
    method: &str,
    path: &str,
    expires_at: i64,
    content_length: Option<u64>,
    signature: &str,
    now: i64,
) -> Result<(), UrlRejection> {
    if now >= expires_at {
        return Err(UrlRejection::Expired);
    }
    let given = base64url::decode(signature).ok_or(UrlRejection::BadSignature)?;
    let text = message(method, path, expires_at, content_length);
    let valid = keys.iter().fold(false, |ok, key| {
        mac(key, &text).verify_slice(&given).is_ok() | ok
    });
    if valid {
        Ok(())
    } else {
        Err(UrlRejection::BadSignature)
    }
}
