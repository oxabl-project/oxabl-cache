//! Identifiers in URLs and JSON. An id that does not parse cannot exist, so it is a 404.

use crate::error::ApiError;
use oxabl_cache_core::encoding::hex_decode;
use oxabl_cache_core::ids::{CacheId, CredentialId, PackId, PrincipalId, TenantId};

pub(crate) fn cache_id(text: &str) -> Result<CacheId, ApiError> {
    text.parse().map_err(|_| ApiError::not_found())
}

pub(crate) fn pack_id(text: &str) -> Result<PackId, ApiError> {
    text.parse().map_err(|_| ApiError::not_found())
}

pub(crate) fn tenant_id(text: &str) -> Result<TenantId, ApiError> {
    text.parse().map_err(|_| ApiError::not_found())
}

pub(crate) fn credential_id(text: &str) -> Result<CredentialId, ApiError> {
    text.parse().map_err(|_| ApiError::not_found())
}

pub(crate) fn principal_id(text: &str) -> Result<PrincipalId, ApiError> {
    text.parse()
        .map_err(|_| ApiError::bad_request("malformed principal id"))
}

/// A 32-byte public key as 64 lowercase hex characters.
pub(crate) fn public_key(text: &str) -> Result<[u8; 32], ApiError> {
    hex_decode(text).ok_or_else(|| ApiError::bad_request("a key is 64 lowercase hex characters"))
}
