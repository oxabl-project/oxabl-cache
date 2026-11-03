//! The garbage-collection leader epoch.

use crate::error::Result;

/// Fencing of garbage-collection transactions.
pub trait FenceTx {
    /// Raises the leader epoch by one and returns the new value.
    fn advance_gc_epoch(&mut self) -> Result<u64>;
    /// Fails with [`Error::Fenced`](crate::Error::Fenced) unless `epoch` is the current leader
    /// epoch. The epoch cannot advance until this transaction ends.
    fn require_gc_epoch(&mut self, epoch: u64) -> Result<()>;
}
