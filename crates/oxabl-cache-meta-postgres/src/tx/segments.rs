//! Segment rows.

use super::{PgTx, RowExt, args, byte_array, int};
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::segment::{Segment, SegmentKind};
use oxabl_cache_core::ids::{CacheId, PackId, SegmentId};
use oxabl_cache_core::ports::metadata::SegmentsTx;
use std::collections::{HashMap, HashSet};
use tokio_postgres::Row;

const SEGMENT_COLUMNS: &str =
    "cache_id, segment_id, pack_id, byte_offset, byte_length, kind, created_at, dead_since";

fn segment_of(row: &Row) -> Result<Segment> {
    Ok(Segment {
        cache: CacheId::from_bytes(row.blob(0)?),
        id: SegmentId::from_bytes(row.blob(1)?),
        pack: PackId::from_bytes(row.blob(2)?),
        offset: row.unsigned(3)?,
        length: row.small(4)?,
        kind: if row.int(5)? == 0 {
            SegmentKind::Artifact
        } else {
            SegmentKind::Dictionary
        },
        created_at: row.int(6)?,
        dead_since: row.opt_int(7)?,
    })
}

fn kind_code(kind: SegmentKind) -> i64 {
    match kind {
        SegmentKind::Artifact => 0,
        SegmentKind::Dictionary => 1,
    }
}

impl SegmentsTx for PgTx<'_> {
    fn segment(&self, cache: &CacheId, id: &SegmentId) -> Result<Option<Segment>> {
        self.query_opt(
            &format!(
                "SELECT {SEGMENT_COLUMNS} FROM segments WHERE cache_id = $1 AND segment_id = $2"
            ),
            args![cache.as_bytes().as_slice(), id.as_bytes().as_slice()],
        )?
        .as_ref()
        .map(segment_of)
        .transpose()
    }

    fn existing_segment_ids(
        &self,
        cache: &CacheId,
        ids: &[SegmentId],
    ) -> Result<HashSet<SegmentId>> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }
        let wanted = byte_array(ids.iter().map(|id| id.as_bytes().as_slice()));
        self.query(
            "SELECT segment_id FROM segments WHERE cache_id = $1 AND segment_id = ANY($2)",
            args![cache.as_bytes().as_slice(), wanted],
        )?
        .iter()
        .map(|row| Ok(SegmentId::from_bytes(row.blob(0)?)))
        .collect()
    }

    fn insert_segment(&mut self, segment: &Segment) -> Result<bool> {
        let changed = self.execute(
            &format!(
                "INSERT INTO segments ({SEGMENT_COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (cache_id, segment_id) DO NOTHING"
            ),
            args![
                segment.cache.as_bytes().as_slice(),
                segment.id.as_bytes().as_slice(),
                segment.pack.as_bytes().as_slice(),
                int(segment.offset),
                i64::from(segment.length),
                kind_code(segment.kind),
                segment.created_at,
                segment.dead_since,
            ],
        )?;
        Ok(changed == 1)
    }

    fn kill_segment(
        &mut self,
        cache: &CacheId,
        id: &SegmentId,
        now: i64,
    ) -> Result<Option<Segment>> {
        self.query_opt(
            &format!(
                "UPDATE segments SET dead_since = $3
                 WHERE cache_id = $1 AND segment_id = $2 AND dead_since IS NULL
                 RETURNING {SEGMENT_COLUMNS}"
            ),
            args![cache.as_bytes().as_slice(), id.as_bytes().as_slice(), now],
        )?
        .as_ref()
        .map(segment_of)
        .transpose()
    }

    fn live_dictionaries(
        &self,
        cache: &CacheId,
        ids: &[SegmentId],
    ) -> Result<HashMap<SegmentId, Segment>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let wanted = byte_array(ids.iter().map(|id| id.as_bytes().as_slice()));
        self.query(
            &format!(
                "SELECT {SEGMENT_COLUMNS} FROM segments
                 WHERE cache_id = $1 AND kind = 1 AND dead_since IS NULL AND segment_id = ANY($2)"
            ),
            args![cache.as_bytes().as_slice(), wanted],
        )?
        .iter()
        .map(|row| segment_of(row).map(|segment| (segment.id, segment)))
        .collect()
    }

    fn relocate_segment(
        &mut self,
        cache: &CacheId,
        id: &SegmentId,
        from: &PackId,
        to: &PackId,
        offset: u64,
    ) -> Result<bool> {
        let changed = self.execute(
            "UPDATE segments SET pack_id = $4, byte_offset = $5
             WHERE cache_id = $1 AND segment_id = $2 AND pack_id = $3 AND dead_since IS NULL",
            args![
                cache.as_bytes().as_slice(),
                id.as_bytes().as_slice(),
                from.as_bytes().as_slice(),
                to.as_bytes().as_slice(),
                int(offset)
            ],
        )?;
        Ok(changed == 1)
    }

    fn segments_of_pack(&self, cache: &CacheId, pack: &PackId) -> Result<Vec<Segment>> {
        self.query(
            &format!(
                "SELECT {SEGMENT_COLUMNS} FROM segments
                 WHERE cache_id = $1 AND pack_id = $2 ORDER BY byte_offset"
            ),
            args![cache.as_bytes().as_slice(), pack.as_bytes().as_slice()],
        )?
        .iter()
        .map(segment_of)
        .collect()
    }

    fn delete_dead_segments_of_pack(&mut self, cache: &CacheId, pack: &PackId) -> Result<u64> {
        self.execute(
            "DELETE FROM segments
             WHERE cache_id = $1 AND pack_id = $2 AND dead_since IS NOT NULL",
            args![cache.as_bytes().as_slice(), pack.as_bytes().as_slice()],
        )
    }

    fn unreferenced_dictionaries(
        &self,
        created_before: i64,
        limit: usize,
    ) -> Result<Vec<(CacheId, SegmentId)>> {
        self.query(
            "SELECT s.cache_id, s.segment_id FROM segments s
             WHERE s.kind = 1 AND s.dead_since IS NULL AND s.created_at <= $1
               AND NOT EXISTS (SELECT 1 FROM dictionary_pointers d
                                WHERE d.cache_id = s.cache_id AND d.segment_id = s.segment_id)
               AND NOT EXISTS (SELECT 1 FROM entries e
                                WHERE e.cache_id = s.cache_id AND e.dict_id = s.segment_id)
             ORDER BY s.created_at LIMIT $2",
            args![created_before, int(limit as u64)],
        )?
        .iter()
        .map(|row| {
            Ok((
                CacheId::from_bytes(row.blob(0)?),
                SegmentId::from_bytes(row.blob(1)?),
            ))
        })
        .collect()
    }
}
