//! In-memory usage counters of this node, flushed serially to the ledger.

use crate::domain::usage::{Metric, UsageRow};
use crate::ids::{CacheId, NodeId, TenantId};
use std::collections::BTreeMap;
use std::sync::Mutex;

type CounterKey = (TenantId, CacheId, i64, Metric);

/// A flush that has been drawn but not yet acknowledged.
#[derive(Debug, Clone)]
pub(crate) struct PendingFlush {
    pub node: NodeId,
    pub seq: u64,
    pub rows: Vec<UsageRow>,
}

#[derive(Debug, Default)]
struct Inner {
    counters: BTreeMap<CounterKey, u64>,
    in_flight: Option<PendingFlush>,
    last_seq: u64,
}

/// Counters of one node. The node id is drawn at process start and never reused.
#[derive(Debug)]
pub struct UsageRecorder {
    node: NodeId,
    inner: Mutex<Inner>,
}

impl UsageRecorder {
    pub(crate) fn new() -> Self {
        Self {
            node: NodeId::random(),
            inner: Mutex::new(Inner::default()),
        }
    }

    /// The id of this node in the ledger.
    pub fn node(&self) -> NodeId {
        self.node
    }

    pub(crate) fn add(
        &self,
        tenant: TenantId,
        cache: CacheId,
        day: i64,
        metric: Metric,
        value: u64,
    ) {
        if value == 0 {
            return;
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *inner
            .counters
            .entry((tenant, cache, day, metric))
            .or_insert(0) += value;
    }

    /// The unacknowledged flush if there is one, else a new flush of the
    /// counters gathered so far, else `None`.
    pub(crate) fn begin_flush(&self) -> Option<PendingFlush> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pending) = &inner.in_flight {
            return Some(pending.clone());
        }
        if inner.counters.is_empty() {
            return None;
        }
        let rows = std::mem::take(&mut inner.counters)
            .into_iter()
            .map(|((tenant, cache, day, metric), value)| UsageRow {
                tenant,
                cache,
                day,
                metric,
                value,
            })
            .collect();
        inner.last_seq += 1;
        let pending = PendingFlush {
            node: self.node,
            seq: inner.last_seq,
            rows,
        };
        inner.in_flight = Some(pending.clone());
        Some(pending)
    }

    pub(crate) fn acknowledge(&self, seq: u64) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.in_flight.as_ref().is_some_and(|p| p.seq == seq) {
            inner.in_flight = None;
        }
    }
}
