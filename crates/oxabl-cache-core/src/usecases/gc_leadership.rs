//! Garbage-collection leadership: the epoch every GC transaction is fenced by.

use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ports::{Clock, MetaTx, MetadataStore, ObjectStore, PackAssembler, TxScope};
use std::sync::atomic::Ordering;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Takes over garbage collection: raises the leader epoch and fences every
    /// older leader's transactions. Waits for their in-flight transactions to end.
    pub async fn assume_gc_leadership(&self) -> Result<u64> {
        let epoch = self
            .meta
            .transact_in(TxScope::Independent, |tx| tx.advance_gc_epoch())
            .await?;
        self.gc_epoch.store(epoch, Ordering::SeqCst);
        Ok(epoch)
    }

    /// Stops garbage collection on this node; its GC transactions fail until it leads again.
    pub fn resign_gc_leadership(&self) {
        self.gc_epoch.store(0, Ordering::SeqCst);
    }

    /// Runs a garbage-collection transaction, refused unless this node still holds
    /// the newest leader epoch when the transaction takes its locks.
    pub(crate) async fn gc_transact_in<T, F>(&self, scope: TxScope, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static,
    {
        let epoch = self.gc_epoch.load(Ordering::SeqCst);
        if epoch == 0 {
            return Err(Error::Fenced);
        }
        self.meta
            .transact_in(scope, move |tx| {
                tx.require_gc_epoch(epoch)?;
                work(tx)
            })
            .await
    }

    /// Fails unless this node still holds the newest leader epoch.
    pub(crate) async fn require_gc_leadership(&self) -> Result<()> {
        self.gc_transact_in(TxScope::Independent, |_| Ok(())).await
    }
}
