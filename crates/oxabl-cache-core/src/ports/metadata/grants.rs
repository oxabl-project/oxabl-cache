//! Grants.

use crate::domain::scope::Grant;
use crate::error::Result;
use crate::ids::{PrincipalId, TenantId};

/// Standing grants.
pub trait GrantsTx {
    /// Replaces the principal's grant for `grant.cache`.
    fn set_grant(
        &mut self,
        tenant: &TenantId,
        principal: &PrincipalId,
        grant: &Grant,
    ) -> Result<()>;
    /// The principal's standing grants.
    fn grants_of(&self, principal: &PrincipalId) -> Result<Vec<Grant>>;
}
