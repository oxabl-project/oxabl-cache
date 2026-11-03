//! The scrub queue.

use crate::error::Result;
use crate::ids::{CacheId, PackId};

/// The scrub queue.
pub trait ScrubQueueTx {
    /// Queues a pack for a full re-verification.
    fn queue_scrub(&mut self, cache: &CacheId, id: &PackId, now: i64) -> Result<()>;
    /// Removes and returns up to `limit` queued scrubs.
    fn take_scrubs(&mut self, limit: usize) -> Result<Vec<(CacheId, PackId)>>;
}
