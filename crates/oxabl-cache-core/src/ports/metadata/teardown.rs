//! Tombstones and the asynchronous removal of deleted tenants and caches.

use crate::error::Result;
use crate::ids::{CacheId, TenantId};

/// Deleting tenants and caches: a tombstone at once, then removal of the rows in batches.
pub trait TeardownTx {
    /// Tombstones a cache of the tenant; `false` if it is unknown or already tombstoned.
    fn tombstone_cache(&mut self, tenant: &TenantId, cache: &CacheId, now: i64) -> Result<bool>;
    /// Tombstones a tenant and every one of its caches; `false` if it is unknown or
    /// already tombstoned.
    fn tombstone_tenant(&mut self, tenant: &TenantId, now: i64) -> Result<bool>;
    /// Whether a tenant has this name, tombstoned tenants included.
    fn tenant_name_taken(&self, name: &str) -> Result<bool>;
    /// Whether the tenant has a cache of this name, tombstoned caches included.
    fn cache_name_taken(&self, tenant: &TenantId, name: &str) -> Result<bool>;
    /// Caches tombstoned at or before `before`, with their tenants.
    fn tombstoned_caches(&self, before: i64, limit: usize) -> Result<Vec<(CacheId, TenantId)>>;
    /// Tombstoned tenants, tombstoned at or before `before`, that own no cache rows.
    fn tombstoned_tenants_without_caches(&self, before: i64, limit: usize)
    -> Result<Vec<TenantId>>;
    /// Deletes up to `limit` rows that depend on a tombstoned cache, then the cache
    /// row itself. Returns `true` once the cache row is gone.
    fn drop_cache_rows(&mut self, cache: &CacheId, limit: usize) -> Result<bool>;
    /// Deletes up to `limit` rows that depend on a tombstoned tenant without caches,
    /// then the tenant row itself. Returns `true` once the tenant row is gone.
    fn drop_tenant_rows(&mut self, tenant: &TenantId, limit: usize) -> Result<bool>;
}
