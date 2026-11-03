//! The service that owns the ports and runs every use case.

use crate::config::Config;
use crate::domain::usage::Metric;
use crate::ids::{CacheId, TenantId};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::state::{AccessBatch, ReportLimiter, UsageRecorder};
use crate::time::day_of;
use std::sync::atomic::AtomicU64;

/// The cache service: ports, settings and per-node state. Use cases are methods
/// spread over the `usecases` modules.
#[derive(Debug)]
pub struct Engine<M, S, A, C> {
    pub(crate) meta: M,
    pub(crate) store: S,
    pub(crate) assembler: A,
    pub(crate) clock: C,
    pub(crate) config: Config,
    pub(crate) usage: UsageRecorder,
    pub(crate) access: AccessBatch,
    pub(crate) reports: ReportLimiter,
    pub(crate) gc_epoch: AtomicU64,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Wires the ports into a service.
    pub fn new(meta: M, store: S, assembler: A, clock: C, config: Config) -> Self {
        Self {
            meta,
            store,
            assembler,
            clock,
            config,
            usage: UsageRecorder::new(),
            access: AccessBatch::default(),
            reports: ReportLimiter::default(),
            gc_epoch: AtomicU64::new(0),
        }
    }

    /// The settings in force.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The metadata port.
    pub fn metadata(&self) -> &M {
        &self.meta
    }

    /// The object store port.
    pub fn object_store(&self) -> &S {
        &self.store
    }

    /// The clock.
    pub fn clock(&self) -> &C {
        &self.clock
    }

    /// The usage recorder of this node.
    pub fn usage(&self) -> &UsageRecorder {
        &self.usage
    }

    pub(crate) fn now(&self) -> i64 {
        self.clock.now()
    }

    pub(crate) fn count(&self, tenant: TenantId, cache: CacheId, metric: Metric, value: u64) {
        self.usage
            .add(tenant, cache, day_of(self.now()), metric, value);
    }
}
