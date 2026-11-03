//! Re-hashing the live segments of a pack and removing what is corrupt.

use crate::domain::pack::PackState;
use crate::domain::segment::Segment;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::{CacheId, PackId, SegmentId};
use crate::ports::TxScope;
use crate::ports::{Clock, MetaTx, MetadataStore, ObjectKey, ObjectStore, PackAssembler};
use crate::usecases::entry_removal::remove_entries;
use crate::usecases::segment_death::kill_segment;

/// What a verification found and removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VerifyOutcome {
    /// Live segments whose bytes do not hash to their id, or that are missing.
    pub corrupt_segments: u64,
    /// Entries deleted because they named a corrupt segment.
    pub entries_removed: u64,
}

/// Deletes every entry naming the segment, drops the dictionary pointer if it names
/// the segment, then kills the segment itself.
pub(crate) fn condemn_segment(
    tx: &mut dyn MetaTx,
    cache: &CacheId,
    id: &SegmentId,
    now: i64,
) -> Result<u64> {
    let entries = tx.entries_naming_segment(cache, id)?;
    let removal = remove_entries(tx, cache, &entries, now, |_| false)?;
    if tx
        .dictionary_pointer(cache)?
        .is_some_and(|pointer| pointer.segment_id == *id)
    {
        tx.delete_dictionary_pointer(cache)?;
    }
    kill_segment(tx, cache, id, now)?;
    Ok(removal.entries)
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Whether the stored bytes of a segment hash to its id. A missing object is corrupt.
    pub(crate) async fn segment_intact(&self, key: &ObjectKey, segment: &Segment) -> Result<bool> {
        match self
            .store
            .read_range(key, segment.offset, u64::from(segment.length), None)
            .await
        {
            Ok(bytes) => Ok(SegmentId::of_bytes(&bytes) == segment.id),
            Err(Error::NotFound) => Ok(false),
            Err(other) => Err(other),
        }
    }

    /// Re-hashes every live segment of a committed pack and removes the entries
    /// that name corrupt ones. The recorded ETag is then refreshed: every live
    /// segment has been proven by its hash, so a replaced object is trusted again.
    pub async fn verify_pack(&self, cache: CacheId, pack: PackId) -> Result<VerifyOutcome> {
        self.verify(cache, pack, false).await
    }

    /// [`Engine::verify_pack`]; when `fenced`, entries are removed only while this
    /// node leads garbage collection.
    pub(crate) async fn verify(
        &self,
        cache: CacheId,
        pack: PackId,
        fenced: bool,
    ) -> Result<VerifyOutcome> {
        let live = self
            .meta
            .read(move |tx| {
                let Some(row) = tx.pack(&cache, &pack)? else {
                    return Ok(Vec::new());
                };
                if row.state != PackState::Live {
                    return Ok(Vec::new());
                }
                Ok(tx
                    .segments_of_pack(&cache, &pack)?
                    .into_iter()
                    .filter(|segment| segment.dead_since.is_none())
                    .collect())
            })
            .await?;
        let key = ObjectKey { cache, pack };
        let head = self.store.head(&key).await?;
        let mut corrupt = Vec::new();
        for segment in &live {
            if !self.segment_intact(&key, segment).await? {
                corrupt.push(segment.id);
            }
        }
        let now = self.now();
        let count = u64::try_from(corrupt.len()).unwrap_or(u64::MAX);
        let write = move |tx: &mut dyn MetaTx| {
            let mut removed = 0;
            for id in &corrupt {
                removed += condemn_segment(tx, &cache, id, now)?;
            }
            if let Some(head) = head {
                tx.set_pack_etag(&cache, &pack, &head.etag)?;
            }
            Ok(removed)
        };
        let scope = TxScope::Cache(cache);
        let entries_removed = if fenced {
            self.gc_transact_in(scope, write).await?
        } else {
            self.meta.transact_in(scope, write).await?
        };
        Ok(VerifyOutcome {
            corrupt_segments: count,
            entries_removed,
        })
    }
}
