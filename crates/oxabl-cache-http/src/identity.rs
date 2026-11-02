//! The identity verifier of a build without OIDC.

use oxabl_cache_core::ids::TenantId;
use oxabl_cache_core::ports::{IdentityVerifier, VerifiedIdentity};
use oxabl_cache_core::{Error, Result};

/// Refuses every token as unavailable; the exchange route answers "not configured".
#[derive(Debug, Clone, Copy, Default)]
pub struct NoIdentity;

impl IdentityVerifier for NoIdentity {
    async fn verify(&self, _tenant: &TenantId, _id_token: &str) -> Result<VerifiedIdentity> {
        Err(Error::unavailable("OIDC is not configured"))
    }
}
