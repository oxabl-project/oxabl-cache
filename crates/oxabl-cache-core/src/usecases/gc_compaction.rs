//! Compaction: copy the live segments of mostly dead packs into a new pack.

use crate::domain::pack::{Pack, PackState};
use crate::domain::segment::Segment;
use crate::domain::usage::Metric;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::{ClientRef, PackId, PrincipalId, SegmentId};
use crate::ports::TxScope;
use crate::ports::{Clock, MetadataStore, ObjectKey, ObjectStore, PackAssembler, PackPart};
use crate::usecases::pack_verify::condemn_segment;

/// What a compaction pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompactionReport {
    /// Source packs emptied into a new pack.
    pub packs: u64,
    /// Segment bytes moved.
    pub bytes: u64,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Compacts live packs older than the grace whose live share is below the
    /// configured ratio. Each live segment is read with `If-Match` on the recorded
    /// ETag and re-hashed against its id; corrupt ones are removed instead of
    /// copied. One transaction then moves the locations and kills the source;
    /// the source pack's bytes stay until the sweep, after the grace.
    pub async fn gc_compaction(&self) -> Result<CompactionReport> {
        let now = self.now();
        let (grace, permille, min_size, batch) = (
            self.config.grace_secs,
            self.config.compaction_live_permille,
            self.config.compaction_min_pack_bytes,
            self.config.gc_batch,
        );
        let candidates = self
            .meta
            .read(move |tx| tx.compaction_candidates(now - grace, permille, min_size, batch))
            .await?;
        let mut report = CompactionReport::default();
        for pack in candidates {
            if let Some(moved) = self.compact_pack(pack).await? {
                report.packs += 1;
                report.bytes += moved;
            }
        }
        Ok(report)
    }

    async fn compact_pack(&self, source: Pack) -> Result<Option<u64>> {
        let (cache, source_id) = (source.cache, source.id);
        let live: Vec<Segment> = self
            .meta
            .read(move |tx| {
                Ok(tx
                    .segments_of_pack(&cache, &source_id)?
                    .into_iter()
                    .filter(|segment| segment.dead_since.is_none())
                    .collect())
            })
            .await?;
        let key = ObjectKey {
            cache,
            pack: source_id,
        };
        let mut parts = Vec::with_capacity(live.len());
        let mut corrupt: Vec<SegmentId> = Vec::new();
        for segment in &live {
            match self
                .store
                .read_range(
                    &key,
                    segment.offset,
                    u64::from(segment.length),
                    source.etag.as_deref(),
                )
                .await
            {
                Ok(bytes) if SegmentId::of_bytes(&bytes) == segment.id => parts.push(PackPart {
                    id: segment.id,
                    bytes,
                }),
                Ok(_) | Err(Error::NotFound) => corrupt.push(segment.id),
                Err(Error::Conflict(_)) => {
                    let now = self.now();
                    self.meta
                        .transact_in(TxScope::Cache(cache), move |tx| {
                            tx.queue_scrub(&cache, &source_id, now)
                        })
                        .await?;
                    return Ok(None);
                }
                Err(other) => return Err(other),
            }
        }
        let now = self.now();
        if !corrupt.is_empty() {
            self.meta
                .transact_in(TxScope::Cache(cache), move |tx| {
                    for id in &corrupt {
                        condemn_segment(tx, &cache, id, now)?;
                    }
                    Ok(())
                })
                .await?;
        }
        if parts.is_empty() {
            return Ok(None);
        }
        let assembled = self.assembler.assemble(&cache, parts)?;
        let new_id = PackId::random();
        let size = u64::try_from(assembled.bytes.len())
            .map_err(|_| Error::internal("assembled pack size overflow"))?;
        let head = self
            .store
            .put(
                &ObjectKey {
                    cache,
                    pack: new_id,
                },
                assembled.bytes,
            )
            .await?;
        let namespace = source.namespace;
        let placements = assembled.placements;
        let now = self.now();
        let moved = self
            .meta
            .transact_in(TxScope::Cache(cache), move |tx| {
                let current = tx.pack(&cache, &source_id)?.ok_or(Error::NotFound)?;
                if current.state != PackState::Live {
                    return Ok(0);
                }
                tx.insert_pack(&Pack {
                    cache,
                    id: new_id,
                    namespace,
                    uploader: PrincipalId::from_bytes([0; PrincipalId::LEN]),
                    client_ref: ClientRef::random(),
                    size,
                    etag: Some(head.etag),
                    live_bytes: 0,
                    state: PackState::Live,
                    lease_until: now,
                    created_at: now,
                    dead_since: None,
                    commit_digest: Some([0; 32]),
                    commit_statuses: Some(Vec::new()),
                })?;
                let mut moved = 0u64;
                for placement in &placements {
                    if tx.relocate_segment(
                        &cache,
                        &placement.id,
                        &source_id,
                        &new_id,
                        placement.offset,
                    )? {
                        moved += u64::from(placement.length);
                    }
                }
                let moved_delta =
                    i64::try_from(moved).map_err(|_| Error::internal("moved bytes overflow"))?;
                if moved == 0 {
                    tx.mark_pack_dead(&cache, &new_id, now)?;
                } else {
                    tx.adjust_pack_live(&cache, &new_id, moved_delta)?;
                    if tx.adjust_pack_live(&cache, &source_id, -moved_delta)? == 0 {
                        tx.mark_pack_dead(&cache, &source_id, now)?;
                    }
                }
                Ok(moved)
            })
            .await?;
        if moved == 0 {
            return Ok(None);
        }
        let owner = self.meta.read(move |tx| tx.cache_tenant(&cache)).await?;
        if let Some(tenant) = owner {
            self.count(tenant, cache, Metric::CompactionBytes, moved);
        }
        Ok(Some(moved))
    }
}
