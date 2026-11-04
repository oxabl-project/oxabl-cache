//! A metadata port that can lose the acknowledgement of a committed transaction.

use oxabl_cache_core::ports::{MetaTx, MetadataStore, TxScope};
use oxabl_cache_core::{Error, Result};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Wraps a real store; when armed, the next transaction commits and then reports failure.
#[derive(Debug, Clone)]
pub struct FlakyMeta<M> {
    inner: M,
    lose_next_ack: Arc<AtomicBool>,
}

impl<M> FlakyMeta<M> {
    pub fn new(inner: M) -> Self {
        Self {
            inner,
            lose_next_ack: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn lose_next_ack(&self) {
        self.lose_next_ack.store(true, Ordering::SeqCst);
    }
}

impl<M: MetadataStore> MetadataStore for FlakyMeta<M> {
    async fn transact_in<T, F>(&self, scope: TxScope, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static,
    {
        let outcome = self.inner.transact_in(scope, work).await?;
        if self.lose_next_ack.swap(false, Ordering::SeqCst) {
            return Err(Error::unavailable("acknowledgement lost"));
        }
        Ok(outcome)
    }
}
