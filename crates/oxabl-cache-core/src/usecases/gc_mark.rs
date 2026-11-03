//! Mark: dictionaries nothing references, and packs with no live bytes left.

use crate::engine::Engine;
use crate::error::Result;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler, TxScope};
use crate::usecases::segment_death::kill_segment;

/// What a mark pass changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MarkReport {
    /// Dictionary segments marked dead.
    pub dictionaries: u64,
    /// Packs marked dead.
    pub packs: u64,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Marks dictionary segments dead that were committed but never referenced
    /// within the grace, and packs left with no live bytes. Most transitions
    /// happen in the transaction that causes them; this pass is the safety net.
    pub async fn gc_mark(&self) -> Result<MarkReport> {
        let now = self.now();
        let grace = self.config.grace_secs;
        let batch = self.config.gc_batch;
        self.gc_transact_in(TxScope::Global, move |tx| {
            let mut report = MarkReport::default();
            for (cache, id) in tx.unreferenced_dictionaries(now - grace, batch)? {
                if kill_segment(tx, &cache, &id, now)? > 0 {
                    report.dictionaries += 1;
                }
            }
            for pack in tx.empty_unmarked_packs(batch)? {
                if tx.mark_pack_dead(&pack.cache, &pack.id, now)? {
                    report.packs += 1;
                }
            }
            Ok(report)
        })
        .await
    }
}
