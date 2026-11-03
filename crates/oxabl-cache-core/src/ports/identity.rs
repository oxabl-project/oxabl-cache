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
    /// The scalar claims a trust policy may match, as name and value.
    pub claims: Vec<(String, String)>,
}

/// Validates OIDC identity tokens and vets issuers.
pub trait IdentityVerifier: Send + Sync + 'static {
    /// Verifies signature, algorithm, issuer, audience and times against the
    /// tenant's `issuers`; fails closed. A token that is not valid is
    /// `Unauthenticated`, an issuer whose keys cannot be fetched `Unavailable`.
    fn verify(
        &self,
        tenant: &TenantId,
        issuers: &[String],
        id_token: &str,
    ) -> impl Future<Output = Result<VerifiedIdentity>> + Send;

    /// Checks that an issuer may be registered: the operator allows it and its
    /// discovery and key documents can be fetched safely. A refusal is `BadRequest`.
    fn probe_issuer(&self, issuer: &str) -> impl Future<Output = Result<()>> + Send;
}
