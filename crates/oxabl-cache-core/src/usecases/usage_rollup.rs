//! Daily ledger maintenance: closing days and snapshotting stored bytes.

use crate::domain::usage::{Metric, ROLLED_UP_NODE, UsageRow};
use crate::engine::Engine;
use crate::error::Result;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::time::{DAY_SECS, day_of};

const CLOSE_DELAY_SECS: i64 = 60 * 60;
const NODE_RETENTION_SECS: i64 = 2 * DAY_SECS;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Closes every day that ended more than an hour ago and rolls its rows up to
    /// one row per tenant, cache and metric. Prunes high-water marks of nodes
    /// silent for two days. Returns the days closed.
    pub async fn close_ledger_days(&self) -> Result<Vec<i64>> {
        let now = self.now();
        self.meta
            .transact(move |tx| {
                let mut closed = Vec::new();
                for day in tx.open_days()? {
                    if now >= (day + 1) * DAY_SECS + CLOSE_DELAY_SECS {
                        tx.roll_up_day(day, now)?;
                        closed.push(day);
                    }
                }
                tx.prune_ledger_nodes(now - NODE_RETENTION_SECS)?;
                Ok(closed)
            })
            .await
    }

    /// Records today's stored-byte snapshot for every cache that has none yet.
    /// Returns how many snapshots were written.
    pub async fn snapshot_stored_bytes(&self) -> Result<u64> {
        let now = self.now();
        let today = day_of(now);
        self.meta
            .transact(move |tx| {
                let mut written = 0;
                for cache in tx.all_caches()? {
                    if tx.has_ledger_row(&cache.id, today, Metric::StoredByteDays)? {
                        continue;
                    }
                    let stored = tx.pack_totals(&cache.id)?.stored_bytes;
                    let row = UsageRow {
                        tenant: cache.tenant,
                        cache: cache.id,
                        day: today,
                        metric: Metric::StoredByteDays,
                        value: stored,
                    };
                    tx.insert_ledger_rows(&ROLLED_UP_NODE, 0, &[row])?;
                    written += 1;
                }
                Ok(written)
            })
            .await
    }
}
