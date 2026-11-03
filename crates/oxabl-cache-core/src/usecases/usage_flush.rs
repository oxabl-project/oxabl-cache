//! Flushing this node's counters to the ledger, serially and idempotently.

use crate::engine::Engine;
use crate::error::Result;
use crate::ports::TxScope;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use std::collections::HashMap;

/// What a flush did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FlushReport {
    /// Rows appended to the ledger.
    pub written: u64,
    /// The flush had been written already and was acknowledged without effect.
    pub ignored: bool,
    /// Rows refused because their day is closed.
    pub late_dropped: u64,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Appends the counters gathered since the last acknowledged flush. A failed
    /// flush is repeated with the same sequence number and rows; counters that
    /// arrive meanwhile wait for the next one. A flush whose sequence is not above
    /// the node's high-water mark is acknowledged and ignored.
    pub async fn flush_usage(&self) -> Result<FlushReport> {
        let Some(pending) = self.usage.begin_flush() else {
            return Ok(FlushReport::default());
        };
        let now = self.now();
        let (node, seq) = (pending.node, pending.seq);
        let report = self
            .meta
            .transact_in(TxScope::Ledger, move |tx| {
                if tx.ledger_high_water(&node)?.is_some_and(|mark| seq <= mark) {
                    return Ok(FlushReport {
                        ignored: true,
                        ..FlushReport::default()
                    });
                }
                let mut closed: HashMap<i64, bool> = HashMap::new();
                let mut open_rows = Vec::new();
                let mut report = FlushReport::default();
                for row in pending.rows {
                    let day_closed = match closed.get(&row.day) {
                        Some(known) => *known,
                        None => {
                            let known = tx.day_closed(row.day)?;
                            closed.insert(row.day, known);
                            known
                        }
                    };
                    if day_closed {
                        report.late_dropped += 1;
                    } else {
                        open_rows.push(row);
                    }
                }
                tx.insert_ledger_rows(&node, seq, &open_rows)?;
                tx.set_ledger_high_water(&node, seq, now)?;
                report.written = u64::try_from(open_rows.len()).unwrap_or(u64::MAX);
                Ok(report)
            })
            .await?;
        self.usage.acknowledge(seq);
        Ok(report)
    }
}
