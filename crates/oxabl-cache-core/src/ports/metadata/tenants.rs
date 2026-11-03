//! Tenants.

use crate::domain::tenant::{Tenant, TenantStatus};
use crate::error::Result;
use crate::ids::TenantId;

/// Tenant rows.
pub trait TenantsTx {
    /// A tenant by id.
    fn tenant(&self, id: &TenantId) -> Result<Option<Tenant>>;
    /// Inserts a tenant; `false` if the id exists.
    fn insert_tenant(&mut self, tenant: &Tenant) -> Result<bool>;
    /// Sets a tenant's status; `false` if it does not exist.
    fn set_tenant_status(&mut self, id: &TenantId, status: TenantStatus) -> Result<bool>;
    /// Live and pending bytes summed over the tenant's caches.
    fn tenant_bytes(&self, tenant: &TenantId) -> Result<(u64, u64)>;
}
