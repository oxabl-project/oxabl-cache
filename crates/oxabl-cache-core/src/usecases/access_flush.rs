//! Writing batched access days, so a hit touches an entry once a day.

use crate::domain::namespace::NamespaceKey;
use crate::engine::Engine;
use crate::error::Result;
use crate::ids::{CacheId, LookupId};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler, TxScope};
use std::collections::BTreeMap;

type DayKey = (CacheId, i64);

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Writes the access days gathered by lookups and returns how many entries
    /// were touched. A failure keeps the batch for the next flush.
    pub async fn flush_access_days(&self) -> Result<u64> {
        let taken = self.access.take();
        if taken.is_empty() {
            return Ok(0);
        }
        let mut groups: BTreeMap<DayKey, Vec<(NamespaceKey, LookupId)>> = BTreeMap::new();
        for ((cache, namespace, id), day) in &taken {
            groups
                .entry((*cache, *day))
                .or_default()
                .push((*namespace, *id));
        }
        let count = u64::try_from(taken.len()).unwrap_or(u64::MAX);
        let mut outcome = Ok(());
        for ((cache, day), keys) in groups {
            outcome = self
                .meta
                .transact_in(TxScope::Cache(cache), move |tx| {
                    tx.touch_entries(&cache, day, &keys)
                })
                .await;
            if outcome.is_err() {
                break;
            }
        }
        match outcome {
            Ok(()) => Ok(count),
            Err(error) => {
                self.access.restore(taken);
                Err(error)
            }
        }
    }
}
