//! Verification of external identity tokens.

use crate::error::Result;
use crate::ids::TenantId;
use std::future::Future;

/// What a verified identity token asserts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentity {
    /// The issuer URL.
    pub issuer: String,
    /// The subject of the token.
    pub subject: String,
    /// The claims a trust policy may match, as name and value.
    pub claims: Vec<(String, String)>,
}

/// Validates an OIDC identity token for a tenant.
pub trait IdentityVerifier: Send + Sync + 'static {
    /// Verifies signature, issuer, audience and times; fails closed.
    fn verify(
        &self,
        tenant: &TenantId,
        id_token: &str,
    ) -> impl Future<Output = Result<VerifiedIdentity>> + Send;
}
