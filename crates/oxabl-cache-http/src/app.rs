//! The state every route shares.

use crate::limiter::RateLimiter;
use crate::metrics::Metrics;
use oxabl_cache_core::Engine;
use oxabl_cache_core::ports::{Clock, IdentityVerifier, MetadataStore, ObjectStore, PackAssembler};
use oxabl_cache_core::usecases::FlushReport;
use oxabl_cache_store_fs::FsStore;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The ports a deployment wires into the service.
pub trait Backend: Send + Sync + 'static {
    /// The metadata store.
    type Meta: MetadataStore;
    /// The object store.
    type Store: ObjectStore;
    /// The writer of pack bytes for compaction.
    type Assembler: PackAssembler;
    /// The source of time.
    type Clock: Clock;
    /// The verifier of OIDC identity tokens.
    type Verifier: IdentityVerifier;
}

/// The engine of a backend.
pub type AppEngine<B> = Engine<
    <B as Backend>::Meta,
    <B as Backend>::Store,
    <B as Backend>::Assembler,
    <B as Backend>::Clock,
>;

/// Limits for tenants created without explicit ones.
#[derive(Debug, Clone, Copy)]
pub struct TenantDefaults {
    /// Requests per second.
    pub rate: u32,
    /// Burst size.
    pub burst: u32,
}

/// The engine and the node-local state the routes need.
pub struct App<B: Backend> {
    /// The use cases.
    pub engine: AppEngine<B>,
    /// The identity verifier behind `POST /v1/auth/oidc`.
    pub verifier: B::Verifier,
    /// The local-disk store whose objects the object route serves; `None` for S3.
    pub local: Option<FsStore>,
    /// The metrics registry.
    pub metrics: Arc<Metrics>,
    /// Rates for tenants created without explicit ones.
    pub defaults: TenantDefaults,
    pub(crate) limiter: RateLimiter,
    leader: AtomicBool,
}

impl<B: Backend> App<B> {
    /// Wires the engine and its node-local state.
    pub fn new(
        engine: AppEngine<B>,
        verifier: B::Verifier,
        local: Option<FsStore>,
        metrics: Arc<Metrics>,
        defaults: TenantDefaults,
    ) -> Self {
        Self {
            engine,
            verifier,
            local,
            metrics,
            defaults,
            limiter: RateLimiter::default(),
            leader: AtomicBool::new(false),
        }
    }

    /// Flushes this node's usage counters to the ledger and counts what the ledger
    /// ignored or dropped.
    pub async fn flush_usage(&self) -> oxabl_cache_core::Result<FlushReport> {
        let report = self.engine.flush_usage().await?;
        if report.ignored {
            self.metrics
                .inc("oxabl_cache_ledger_ignored_flushes_total", &[], 1);
        }
        if report.late_dropped > 0 {
            self.metrics.inc(
                "oxabl_cache_ledger_late_dropped_total",
                &[],
                report.late_dropped,
            );
        }
        Ok(report)
    }

    /// Records whether this node runs the garbage collector.
    pub fn set_gc_leader(&self, leader: bool) {
        self.leader.store(leader, Ordering::Relaxed);
    }

    /// Whether this node runs the garbage collector.
    pub fn gc_leader(&self) -> bool {
        self.leader.load(Ordering::Relaxed)
    }
}
