//! Tenants and their quotas.

use crate::ids::TenantId;

/// Whether a tenant may use the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantStatus {
    /// Requests are served.
    Active,
    /// Every credential of the tenant is refused.
    Suspended,
}

/// A customer account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tenant {
    /// The tenant id.
    pub id: TenantId,
    /// Operator-visible name.
    pub name: String,
    /// Whether requests are served.
    pub status: TenantStatus,
    /// Hard limit on live plus pending bytes across the tenant's caches.
    pub hard_bytes: Option<u64>,
    /// Monthly egress limit; fetch-mode lookups are refused above it.
    pub egress_bytes: Option<u64>,
    /// Requests per second on API routes.
    pub rate: u32,
    /// Burst size of the request rate.
    pub burst: u32,
    /// Creation time, unix seconds.
    pub created_at: i64,
}
