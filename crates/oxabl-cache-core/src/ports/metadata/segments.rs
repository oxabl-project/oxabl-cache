//! Segment rows.

use crate::domain::segment::Segment;
use crate::error::Result;
use crate::ids::{CacheId, PackId, SegmentId};
use std::collections::{HashMap, HashSet};

/// Queries and writes of segment rows.
pub trait SegmentsTx {
    /// A segment of a cache, live or dead.
    fn segment(&self, cache: &CacheId, id: &SegmentId) -> Result<Option<Segment>>;
    /// The ids among `ids` already recorded in the cache.
    fn existing_segment_ids(
        &self,
        cache: &CacheId,
        ids: &[SegmentId],
    ) -> Result<HashSet<SegmentId>>;
    /// Inserts a segment; `false`, changing nothing, if its id is recorded.
    fn insert_segment(&mut self, segment: &Segment) -> Result<bool>;
    /// Inserts segments in one round trip where the store can; per segment, `false`
    /// if its id is recorded. The ids of one batch are distinct.
    fn insert_segments(&mut self, segments: &[Segment]) -> Result<Vec<bool>> {
        segments
            .iter()
            .map(|segment| self.insert_segment(segment))
            .collect()
    }
    /// Sets `dead_since` on a live segment and returns it; `None` if it is dead or unknown.
    fn kill_segment(
        &mut self,
        cache: &CacheId,
        id: &SegmentId,
        now: i64,
    ) -> Result<Option<Segment>>;
    /// The live dictionary segments among `ids`, locked against dying until the
    /// transaction ends where the database supports it.
    fn live_dictionaries(
        &self,
        cache: &CacheId,
        ids: &[SegmentId],
    ) -> Result<HashMap<SegmentId, Segment>>;
    /// Points a live segment at its copy in another pack; `false` if it is no
    /// longer live in `from`.
    fn relocate_segment(
        &mut self,
        cache: &CacheId,
        id: &SegmentId,
        from: &PackId,
        to: &PackId,
        offset: u64,
    ) -> Result<bool>;
    /// Every segment row of a pack.
    fn segments_of_pack(&self, cache: &CacheId, pack: &PackId) -> Result<Vec<Segment>>;
    /// Deletes the dead segment rows of a pack and returns how many.
    fn delete_dead_segments_of_pack(&mut self, cache: &CacheId, pack: &PackId) -> Result<u64>;
    /// Live dictionary segments created before `created_before` that neither the
    /// pointer nor any entry names.
    fn unreferenced_dictionaries(
        &self,
        created_before: i64,
        limit: usize,
    ) -> Result<Vec<(CacheId, SegmentId)>>;
}
