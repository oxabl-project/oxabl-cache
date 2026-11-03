//! Credentials.

use crate::domain::credential::{Credential, CredentialKind};
use crate::error::Result;
use crate::ids::{CredentialId, PrincipalId};

/// Tokens and sessions.
pub trait CredentialsTx {
    /// Inserts a credential and its grants; `false` if the id exists.
    fn insert_credential(&mut self, credential: &Credential) -> Result<bool>;
    /// A credential with its grants.
    fn credential(&self, kind: CredentialKind, id: &CredentialId) -> Result<Option<Credential>>;
    /// Revokes a credential; `false` if unknown or already revoked.
    fn revoke_credential(
        &mut self,
        kind: CredentialKind,
        id: &CredentialId,
        now: i64,
    ) -> Result<bool>;
    /// Revokes every credential of a principal and returns how many.
    fn revoke_credentials_of(&mut self, principal: &PrincipalId, now: i64) -> Result<u64>;
}
