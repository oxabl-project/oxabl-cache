//! The authenticated caller.

use crate::domain::scope::{Grant, Scope};
use crate::error::{Error, Result};
use crate::ids::{CacheId, PrincipalId, TenantId};

/// Who calls: the principal, its tenant and what it may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// The tenant every query of this caller is scoped to.
    pub tenant: TenantId,
    /// The user, workload or token owner acting.
    pub principal: PrincipalId,
    /// Standing scopes per cache, and tenant-wide ones.
    pub grants: Vec<Grant>,
}

impl Caller {
    /// Whether the caller holds `scope` on `cache`.
    pub fn can(&self, cache: &CacheId, scope: Scope) -> bool {
        self.grants.iter().any(|grant| {
            grant.scopes.contains(scope) && grant.cache.as_ref().is_some_and(|c| c == cache)
        })
    }

    /// Whether the caller holds `tenant:admin` for its tenant.
    pub fn is_tenant_admin(&self) -> bool {
        self.grants
            .iter()
            .any(|grant| grant.cache.is_none() && grant.scopes.contains(Scope::TenantAdmin))
    }

    /// Refuses unless the caller holds `scope` on `cache`.
    pub fn require(&self, cache: &CacheId, scope: Scope) -> Result<()> {
        if self.can(cache, scope) {
            Ok(())
        } else {
            Err(Error::Forbidden("missing scope"))
        }
    }

    /// Refuses unless the caller holds `tenant:admin`.
    pub fn require_tenant_admin(&self) -> Result<()> {
        if self.is_tenant_admin() {
            Ok(())
        } else {
            Err(Error::Forbidden("missing scope"))
        }
    }
}
