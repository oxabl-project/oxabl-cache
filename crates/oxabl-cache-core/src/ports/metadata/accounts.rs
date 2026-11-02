//! Tenants, caches, users, grants and credentials.

use crate::domain::cache::Cache;
use crate::domain::credential::{Credential, CredentialKind};
use crate::domain::scope::Grant;
use crate::domain::tenant::{Tenant, TenantStatus};
use crate::error::Result;
use crate::ids::{CacheId, CredentialId, PrincipalId, TenantId};

/// A human principal, keyed by issuer and subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// The principal id.
    pub id: PrincipalId,
    /// The tenant.
    pub tenant: TenantId,
    /// Issuer URL of the identity provider.
    pub issuer: String,
    /// Subject at the issuer.
    pub subject: String,
    /// Operator-visible label.
    pub label: String,
    /// Creation time, unix seconds.
    pub created_at: i64,
    /// Set when the user was disabled.
    pub disabled_at: Option<i64>,
}

/// Account-level queries and writes.
pub trait AccountsTx {
    /// A tenant by id.
    fn tenant(&self, id: &TenantId) -> Result<Option<Tenant>>;
    /// Inserts a tenant; `false` if the id exists.
    fn insert_tenant(&mut self, tenant: &Tenant) -> Result<bool>;
    /// Sets a tenant's status; `false` if it does not exist.
    fn set_tenant_status(&mut self, id: &TenantId, status: TenantStatus) -> Result<bool>;
    /// Inserts a cache; `false` if the id exists.
    fn insert_cache(&mut self, cache: &Cache) -> Result<bool>;
    /// A cache of the tenant; another tenant's cache is `None`.
    fn cache(&self, tenant: &TenantId, id: &CacheId) -> Result<Option<Cache>>;
    /// The tenant that owns a cache.
    fn cache_tenant(&self, id: &CacheId) -> Result<Option<TenantId>>;
    /// Every cache of every tenant.
    fn all_caches(&self) -> Result<Vec<Cache>>;
    /// Live and pending bytes summed over the tenant's caches.
    fn tenant_bytes(&self, tenant: &TenantId) -> Result<(u64, u64)>;
    /// Adds signed deltas to a cache's live and pending byte counters; a counter
    /// that would go negative is an internal error.
    fn adjust_cache_bytes(&mut self, cache: &CacheId, live: i64, pending: i64) -> Result<()>;
    /// Inserts a user; `false` if the id or the issuer and subject exist.
    fn insert_user(&mut self, user: &User) -> Result<bool>;
    /// A user by id.
    fn user(&self, id: &PrincipalId) -> Result<Option<User>>;
    /// Marks a user disabled; `false` if unknown or already disabled.
    fn disable_user(&mut self, id: &PrincipalId, now: i64) -> Result<bool>;
    /// Replaces the principal's grant for `grant.cache`.
    fn set_grant(
        &mut self,
        tenant: &TenantId,
        principal: &PrincipalId,
        grant: &Grant,
    ) -> Result<()>;
    /// The principal's standing grants.
    fn grants_of(&self, principal: &PrincipalId) -> Result<Vec<Grant>>;
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
