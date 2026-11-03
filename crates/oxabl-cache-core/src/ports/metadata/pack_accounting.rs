//! Pack accounting.

use crate::domain::namespace::NamespaceKey;
use crate::domain::pack::PackTotals;
use crate::error::Result;
use crate::ids::CacheId;

/// Byte totals and the live-byte audit.
pub trait PackAccountingTx {
    /// Byte totals over the cache's packs.
    fn pack_totals(&self, cache: &CacheId) -> Result<PackTotals>;
    /// Live bytes plus pending sizes of one namespace.
    fn namespace_bytes(&self, cache: &CacheId, namespace: &NamespaceKey) -> Result<u64>;
    /// Recomputes live-byte sums from segment rows and describes each mismatch
    /// with the stored counters of the cache and of its packs.
    fn audit_live_bytes(&self, cache: &CacheId) -> Result<Vec<String>>;
}
