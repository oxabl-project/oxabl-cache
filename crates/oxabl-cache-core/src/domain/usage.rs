//! Usage ledger rows.

use crate::ids::{CacheId, NodeId, TenantId};

/// A counted quantity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Metric {
    /// Daily snapshot of pending, live and in-grace pack bytes.
    StoredByteDays,
    /// Sizes of committed packs.
    IngressBytes,
    /// Segment lengths returned by fetch-mode lookups.
    EgressBytes,
    /// Requests served.
    Requests,
    /// Lookup ids probed.
    LookupIds,
    /// Lookup ids that hit.
    Hits,
    /// Entries committed.
    EntriesCommitted,
    /// Bytes rewritten by compaction.
    CompactionBytes,
}

impl Metric {
    /// Every metric.
    pub const ALL: [Metric; 8] = [
        Metric::StoredByteDays,
        Metric::IngressBytes,
        Metric::EgressBytes,
        Metric::Requests,
        Metric::LookupIds,
        Metric::Hits,
        Metric::EntriesCommitted,
        Metric::CompactionBytes,
    ];

    /// The name used in exports.
    pub const fn name(self) -> &'static str {
        match self {
            Metric::StoredByteDays => "stored_byte_days",
            Metric::IngressBytes => "ingress_bytes",
            Metric::EgressBytes => "egress_bytes",
            Metric::Requests => "requests",
            Metric::LookupIds => "lookup_ids",
            Metric::Hits => "hits",
            Metric::EntriesCommitted => "entries_committed",
            Metric::CompactionBytes => "compaction_bytes",
        }
    }

    /// Parses an export name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|metric| metric.name() == name)
    }
}

/// One ledger row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageRow {
    /// The tenant.
    pub tenant: TenantId,
    /// The cache.
    pub cache: CacheId,
    /// UTC day number.
    pub day: i64,
    /// The quantity.
    pub metric: Metric,
    /// The amount.
    pub value: u64,
}

/// A summed usage figure for one tenant, cache and metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageTotal {
    /// The tenant.
    pub tenant: TenantId,
    /// The cache.
    pub cache: CacheId,
    /// The quantity.
    pub metric: Metric,
    /// The summed amount.
    pub value: u64,
}

/// The ledger id of a node's rows after the daily roll-up.
pub const ROLLED_UP_NODE: NodeId = NodeId::from_bytes([0; NodeId::LEN]);
