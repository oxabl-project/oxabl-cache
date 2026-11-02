//! Listings and operator-level writes behind the administration routes.

use crate::domain::cache::Cache;
use crate::domain::credential::{OperatorToken, TokenSummary};
use crate::domain::scope::Grant;
use crate::domain::tenant::{Tenant, TenantLimits};
use crate::error::Result;
use crate::ids::{CredentialId, PrincipalId, TenantId};
use crate::ports::metadata::User;

/// Queries and writes of tenant and operator administration.
pub trait AdminTx {
    /// Every tenant, ordered by id.
    fn all_tenants(&self) -> Result<Vec<Tenant>>;
    /// Replaces a tenant's limits; `false` if it does not exist.
    fn set_tenant_limits(&mut self, id: &TenantId, limits: &TenantLimits) -> Result<bool>;
    /// The tenant's caches, ordered by id.
    fn caches_of(&self, tenant: &TenantId) -> Result<Vec<Cache>>;
    /// The tenant's API tokens, newest last.
    fn tokens_of(&self, tenant: &TenantId) -> Result<Vec<TokenSummary>>;
    /// The tenant's users, ordered by creation.
    fn users_of(&self, tenant: &TenantId) -> Result<Vec<User>>;
    /// Every standing grant in the tenant, with its principal.
    fn tenant_grants(&self, tenant: &TenantId) -> Result<Vec<(PrincipalId, Grant)>>;
    /// Inserts an operator token; `false` if the id exists.
    fn insert_operator_token(&mut self, token: &OperatorToken) -> Result<bool>;
    /// An operator token by id.
    fn operator_token(&self, id: &CredentialId) -> Result<Option<OperatorToken>>;
    /// Whether any operator token was ever created.
    fn operator_tokens_exist(&self) -> Result<bool>;
}
