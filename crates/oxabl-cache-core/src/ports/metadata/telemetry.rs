//! Sealed telemetry reports of a cache.

use crate::domain::telemetry::StoredTelemetry;
use crate::error::Result;
use crate::ids::CacheId;

/// A bounded log of opaque reports per cache.
pub trait TelemetryTx {
    /// Appends a report and drops the cache's reports beyond the newest `keep` and those
    /// received before `oldest`. Returns the new report's sequence number.
    fn append_telemetry(
        &mut self,
        cache: &CacheId,
        received_at: i64,
        body: &[u8],
        keep: usize,
        oldest: i64,
    ) -> Result<u64>;
    /// Up to `limit` reports with a sequence number above `after`, oldest first.
    fn telemetry_after(
        &self,
        cache: &CacheId,
        after: u64,
        limit: usize,
    ) -> Result<Vec<StoredTelemetry>>;
}
