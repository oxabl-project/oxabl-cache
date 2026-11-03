//! Caches.

use crate::domain::cache::Cache;
use crate::error::Result;
use crate::ids::{CacheId, TenantId};

/// Cache rows and byte counters.
pub trait CachesTx {
    /// Inserts a cache; `false` if the id exists.
    fn insert_cache(&mut self, cache: &Cache) -> Result<bool>;
    /// A cache of the tenant; another tenant's cache is `None`.
    fn cache(&self, tenant: &TenantId, id: &CacheId) -> Result<Option<Cache>>;
    /// The tenant that owns a cache.
    fn cache_tenant(&self, id: &CacheId) -> Result<Option<TenantId>>;
    /// Every cache of every tenant.
    fn all_caches(&self) -> Result<Vec<Cache>>;
    /// Adds signed deltas to a cache's live and pending byte counters; a counter
    /// that would go negative is an internal error.
    fn adjust_cache_bytes(&mut self, cache: &CacheId, live: i64, pending: i64) -> Result<()>;
}
