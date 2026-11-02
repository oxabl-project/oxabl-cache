//! Tokens and sessions.

use crate::domain::scope::Grant;
use crate::ids::{CredentialId, PrincipalId, TenantId};

/// What a credential is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    /// A long-lived API token, `oxc_<id>_<secret>`.
    Token,
    /// A short-lived session, `oxs_<id>_<secret>`.
    Session,
}

impl CredentialKind {
    /// The textual prefix of the credential.
    pub const fn prefix(self) -> &'static str {
        match self {
            CredentialKind::Token => "oxc_",
            CredentialKind::Session => "oxs_",
        }
    }
}

/// A stored credential: only a hash of the secret is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    /// What it is.
    pub kind: CredentialKind,
    /// Public id.
    pub id: CredentialId,
    /// The tenant it belongs to.
    pub tenant: TenantId,
    /// The principal it acts as.
    pub principal: PrincipalId,
    /// Operator-visible label.
    pub name: String,
    /// SHA-256 of the secret.
    pub secret_hash: [u8; 32],
    /// Creation time, unix seconds.
    pub created_at: i64,
    /// Expiry, unix seconds.
    pub expires_at: i64,
    /// Revocation time, if revoked.
    pub revoked_at: Option<i64>,
    /// What the credential may do.
    pub grants: Vec<Grant>,
}
