//! The append-only usage ledger and the node heartbeat.

use crate::domain::usage::{Metric, UsageRow, UsageTotal};
use crate::error::Result;
use crate::ids::{CacheId, NodeId, TenantId};

/// Queries and writes of the usage ledger.
pub trait LedgerTx {
    /// The highest flush sequence recorded for a node.
    fn ledger_high_water(&self, node: &NodeId) -> Result<Option<u64>>;
    /// Raises a node's high-water mark and records when it flushed.
    fn set_ledger_high_water(&mut self, node: &NodeId, seq: u64, now: i64) -> Result<()>;
    /// Whether a day is closed to new rows.
    fn day_closed(&self, day: i64) -> Result<bool>;
    /// Appends rows of one flush.
    fn insert_ledger_rows(&mut self, node: &NodeId, seq: u64, rows: &[UsageRow]) -> Result<()>;
    /// Days that have rows and are not closed.
    fn open_days(&self) -> Result<Vec<i64>>;
    /// Closes a day and replaces its rows with one row per tenant, cache and metric.
    fn roll_up_day(&mut self, day: i64, now: i64) -> Result<()>;
    /// Whether a row exists for a cache, day and metric.
    fn has_ledger_row(&self, cache: &CacheId, day: i64, metric: Metric) -> Result<bool>;
    /// Sums per tenant, cache and metric over the days `[from, to)`.
    fn usage_totals(&self, from: i64, to: i64) -> Result<Vec<UsageTotal>>;
    /// One tenant's sum of one metric over the days `[from, to)`.
    fn tenant_metric(&self, tenant: &TenantId, metric: Metric, from: i64, to: i64) -> Result<u64>;
    /// Removes the high-water marks of nodes that last flushed before `before`.
    fn prune_ledger_nodes(&mut self, before: i64) -> Result<u64>;

    /// Records that a node is alive.
    fn heartbeat(&mut self, node: &NodeId, now: i64) -> Result<()>;
    /// Nodes that beat at or after `since`.
    fn live_nodes(&self, since: i64) -> Result<u32>;
    /// Removes heartbeats older than `before`.
    fn prune_nodes(&mut self, before: i64) -> Result<u64>;
}
