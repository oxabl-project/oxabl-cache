//! Issuers and workload trust policies.

use crate::domain::oidc::{Issuer, WorkloadPolicy};
use crate::error::Result;
use crate::ids::{PrincipalId, TenantId};
use crate::ports::metadata::User;

/// Queries and writes of OIDC trust.
pub trait OidcTx {
    /// Inserts an issuer; `false` if the tenant already registered the URL.
    fn insert_issuer(&mut self, issuer: &Issuer) -> Result<bool>;
    /// The tenant's issuer with this URL.
    fn issuer(&self, tenant: &TenantId, url: &str) -> Result<Option<Issuer>>;
    /// The tenant's issuers, ordered by URL.
    fn issuers_of(&self, tenant: &TenantId) -> Result<Vec<Issuer>>;
    /// Deletes an issuer and its policies; `false` if it was not registered.
    fn delete_issuer(&mut self, tenant: &TenantId, url: &str) -> Result<bool>;
    /// Inserts a policy; `false` if the tenant already has a policy of that name.
    fn insert_policy(&mut self, policy: &WorkloadPolicy) -> Result<bool>;
    /// The tenant's policy of this name.
    fn policy_named(&self, tenant: &TenantId, name: &str) -> Result<Option<WorkloadPolicy>>;
    /// The tenant's policies, ordered by creation then id.
    fn policies_of(&self, tenant: &TenantId) -> Result<Vec<WorkloadPolicy>>;
    /// Deletes a policy; `false` if it does not exist.
    fn delete_policy(&mut self, tenant: &TenantId, id: &PrincipalId) -> Result<bool>;
    /// The tenant's user with this issuer and subject.
    fn user_by_subject(
        &self,
        tenant: &TenantId,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<User>>;
}
