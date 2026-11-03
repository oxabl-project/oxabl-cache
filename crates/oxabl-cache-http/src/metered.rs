//! Ports wrapped to time their calls into the metrics registry.

use crate::metrics::Metrics;
use oxabl_cache_core::Result;
use oxabl_cache_core::ids::CacheId;
use oxabl_cache_core::ports::{
    MetaTx, MetadataStore, ObjectHead, ObjectInfo, ObjectKey, ObjectStore, SignedUrl,
    StoreCapabilities, TxScope,
};
use std::sync::Arc;
use std::time::Instant;

/// A metadata store that records the duration of every transaction.
#[derive(Debug, Clone)]
pub struct MeteredMeta<M> {
    inner: M,
    metrics: Arc<Metrics>,
}

impl<M> MeteredMeta<M> {
    /// Wraps `inner`.
    pub fn new(inner: M, metrics: Arc<Metrics>) -> Self {
        Self { inner, metrics }
    }
}

impl<M: MetadataStore> MetadataStore for MeteredMeta<M> {
    async fn transact_in<T, F>(&self, scope: TxScope, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static,
    {
        let started = Instant::now();
        let outcome = self.inner.transact_in(scope, work).await;
        self.metrics.observe(
            "oxabl_cache_metadata_query_seconds",
            &[("op", "transact")],
            started.elapsed().as_secs_f64(),
        );
        outcome
    }

    async fn read<T, F>(&self, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static,
    {
        let started = Instant::now();
        let outcome = self.inner.read(work).await;
        self.metrics.observe(
            "oxabl_cache_metadata_query_seconds",
            &[("op", "read")],
            started.elapsed().as_secs_f64(),
        );
        outcome
    }
}

/// An object store that records the duration and failures of every call.
#[derive(Debug, Clone)]
pub struct MeteredStore<S> {
    inner: S,
    metrics: Arc<Metrics>,
}

impl<S> MeteredStore<S> {
    /// Wraps `inner`.
    pub fn new(inner: S, metrics: Arc<Metrics>) -> Self {
        Self { inner, metrics }
    }

    fn record<T>(&self, op: &str, started: Instant, outcome: &Result<T>) {
        self.metrics.observe(
            "oxabl_cache_store_request_seconds",
            &[("op", op)],
            started.elapsed().as_secs_f64(),
        );
        if matches!(
            outcome,
            Err(oxabl_cache_core::Error::Unavailable(_) | oxabl_cache_core::Error::Internal(_))
        ) {
            self.metrics
                .inc("oxabl_cache_store_errors_total", &[("op", op)], 1);
        }
    }
}

impl<S: ObjectStore> ObjectStore for MeteredStore<S> {
    fn capabilities(&self) -> StoreCapabilities {
        self.inner.capabilities()
    }

    async fn put_url(
        &self,
        key: &ObjectKey,
        size: u64,
        sha256: Option<[u8; 32]>,
        expires_at: i64,
    ) -> Result<SignedUrl> {
        let started = Instant::now();
        let outcome = self.inner.put_url(key, size, sha256, expires_at).await;
        self.record("put_url", started, &outcome);
        outcome
    }

    async fn get_url(&self, key: &ObjectKey, expires_at: i64) -> Result<SignedUrl> {
        let started = Instant::now();
        let outcome = self.inner.get_url(key, expires_at).await;
        self.record("get_url", started, &outcome);
        outcome
    }

    async fn head(&self, key: &ObjectKey) -> Result<Option<ObjectHead>> {
        let started = Instant::now();
        let outcome = self.inner.head(key).await;
        self.record("head", started, &outcome);
        outcome
    }

    async fn read_range(
        &self,
        key: &ObjectKey,
        offset: u64,
        length: u64,
        if_match: Option<&str>,
    ) -> Result<Vec<u8>> {
        let started = Instant::now();
        let outcome = self.inner.read_range(key, offset, length, if_match).await;
        self.record("read_range", started, &outcome);
        outcome
    }

    async fn put(&self, key: &ObjectKey, bytes: Vec<u8>) -> Result<ObjectHead> {
        let started = Instant::now();
        let outcome = self.inner.put(key, bytes).await;
        self.record("put", started, &outcome);
        outcome
    }

    async fn delete(&self, key: &ObjectKey) -> Result<()> {
        let started = Instant::now();
        let outcome = self.inner.delete(key).await;
        self.record("delete", started, &outcome);
        outcome
    }

    async fn list(&self, cache: &CacheId) -> Result<Vec<ObjectInfo>> {
        let started = Instant::now();
        let outcome = self.inner.list(cache).await;
        self.record("list", started, &outcome);
        outcome
    }
}
