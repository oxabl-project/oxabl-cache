//! Segment rows.

use super::SqliteTx;
use crate::db::{DbResult, RowExt, int, placeholders};
use oxabl_cache_core::domain::segment::{Segment, SegmentKind};
use oxabl_cache_core::ids::{CacheId, PackId, SegmentId};
use oxabl_cache_core::ports::metadata::SegmentsTx;
use oxabl_cache_core::{Error, Result};
use rusqlite::{OptionalExtension, Row, params, params_from_iter};
use std::collections::{HashMap, HashSet};

const CHUNK: usize = 400;
const SEGMENT_COLUMNS: &str =
    "cache_id, segment_id, pack_id, byte_offset, byte_length, kind, created_at, dead_since";

fn segment_of(row: &Row<'_>) -> rusqlite::Result<Segment> {
    let kind: i64 = row.get(5)?;
    Ok(Segment {
        cache: CacheId::from_bytes(row.blob(0)?),
        id: SegmentId::from_bytes(row.blob(1)?),
        pack: PackId::from_bytes(row.blob(2)?),
        offset: row.unsigned(3)?,
        length: row.small(4)?,
        kind: if kind == 0 {
            SegmentKind::Artifact
        } else {
            SegmentKind::Dictionary
        },
        created_at: row.get(6)?,
        dead_since: row.get(7)?,
    })
}

fn kind_code(kind: SegmentKind) -> i64 {
    match kind {
        SegmentKind::Artifact => 0,
        SegmentKind::Dictionary => 1,
    }
}

impl SegmentsTx for SqliteTx<'_> {
    fn segment(&self, cache: &CacheId, id: &SegmentId) -> Result<Option<Segment>> {
        self.conn
            .prepare_cached(&format!(
                "SELECT {SEGMENT_COLUMNS} FROM segments WHERE cache_id = ?1 AND segment_id = ?2"
            ))
            .db()?
            .query_row(
                [cache.as_bytes().as_slice(), id.as_bytes().as_slice()],
                segment_of,
            )
            .optional()
            .db()
    }

    fn existing_segment_ids(
        &self,
        cache: &CacheId,
        ids: &[SegmentId],
    ) -> Result<HashSet<SegmentId>> {
        let mut found = HashSet::new();
        for chunk in ids.chunks(CHUNK) {
            let sql = format!(
                "SELECT segment_id FROM segments WHERE cache_id = ?1 AND segment_id IN ({})",
                placeholders(2, chunk.len())
            );
            let mut statement = self.conn.prepare_cached(&sql).db()?;
            let values = std::iter::once(cache.as_bytes().as_slice())
                .chain(chunk.iter().map(|id| id.as_bytes().as_slice()));
            let rows = statement
                .query_map(params_from_iter(values), |row| {
                    Ok(SegmentId::from_bytes(row.blob(0)?))
                })
                .db()?;
            for row in rows {
                found.insert(row.db()?);
            }
        }
        Ok(found)
    }

    fn insert_segment(&mut self, segment: &Segment) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(&format!(
                "INSERT INTO segments ({SEGMENT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (cache_id, segment_id) DO NOTHING"
            ))
            .db()?
            .execute(params![
                segment.cache.as_bytes().as_slice(),
                segment.id.as_bytes().as_slice(),
                segment.pack.as_bytes().as_slice(),
                int(segment.offset),
                segment.length,
                kind_code(segment.kind),
                segment.created_at,
                segment.dead_since,
            ])
            .db()?;
        Ok(changed == 1)
    }

    fn kill_segment(
        &mut self,
        cache: &CacheId,
        id: &SegmentId,
        now: i64,
    ) -> Result<Option<Segment>> {
        let changed = self
            .conn
            .prepare_cached(
                "UPDATE segments SET dead_since = ?3
                 WHERE cache_id = ?1 AND segment_id = ?2 AND dead_since IS NULL",
            )
            .db()?
            .execute(params![
                cache.as_bytes().as_slice(),
                id.as_bytes().as_slice(),
                now
            ])
            .db()?;
        if changed == 0 {
            return Ok(None);
        }
        let segment = self
            .segment(cache, id)?
            .ok_or_else(|| Error::internal("segment vanished while being killed"))?;
        Ok(Some(segment))
    }

    fn live_dictionaries(
        &self,
        cache: &CacheId,
        ids: &[SegmentId],
    ) -> Result<HashMap<SegmentId, Segment>> {
        let mut found = HashMap::new();
        for chunk in ids.chunks(CHUNK) {
            let sql = format!(
                "SELECT {SEGMENT_COLUMNS} FROM segments
                 WHERE cache_id = ?1 AND kind = 1 AND dead_since IS NULL AND segment_id IN ({})",
                placeholders(2, chunk.len())
            );
            let mut statement = self.conn.prepare_cached(&sql).db()?;
            let values = std::iter::once(cache.as_bytes().as_slice())
                .chain(chunk.iter().map(|id| id.as_bytes().as_slice()));
            let rows = statement
                .query_map(params_from_iter(values), segment_of)
                .db()?;
            for row in rows {
                let segment = row.db()?;
                found.insert(segment.id, segment);
            }
        }
        Ok(found)
    }

    fn relocate_segment(
        &mut self,
        cache: &CacheId,
        id: &SegmentId,
        from: &PackId,
        to: &PackId,
        offset: u64,
    ) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(
                "UPDATE segments SET pack_id = ?4, byte_offset = ?5
                 WHERE cache_id = ?1 AND segment_id = ?2 AND pack_id = ?3 AND dead_since IS NULL",
            )
            .db()?
            .execute(params![
                cache.as_bytes().as_slice(),
                id.as_bytes().as_slice(),
                from.as_bytes().as_slice(),
                to.as_bytes().as_slice(),
                int(offset)
            ])
            .db()?;
        Ok(changed == 1)
    }

    fn segments_of_pack(&self, cache: &CacheId, pack: &PackId) -> Result<Vec<Segment>> {
        let mut statement = self
            .conn
            .prepare_cached(&format!(
                "SELECT {SEGMENT_COLUMNS} FROM segments
                 WHERE cache_id = ?1 AND pack_id = ?2 ORDER BY byte_offset"
            ))
            .db()?;
        let rows = statement
            .query_map(
                [cache.as_bytes().as_slice(), pack.as_bytes().as_slice()],
                segment_of,
            )
            .db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }

    fn delete_dead_segments_of_pack(&mut self, cache: &CacheId, pack: &PackId) -> Result<u64> {
        let deleted = self
            .conn
            .prepare_cached(
                "DELETE FROM segments
                 WHERE cache_id = ?1 AND pack_id = ?2 AND dead_since IS NOT NULL",
            )
            .db()?
            .execute([cache.as_bytes().as_slice(), pack.as_bytes().as_slice()])
            .db()?;
        Ok(u64::try_from(deleted).unwrap_or(u64::MAX))
    }

    fn unreferenced_dictionaries(
        &self,
        created_before: i64,
        limit: usize,
    ) -> Result<Vec<(CacheId, SegmentId)>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT s.cache_id, s.segment_id FROM segments s
                 WHERE s.kind = 1 AND s.dead_since IS NULL AND s.created_at <= ?1
                   AND NOT EXISTS (SELECT 1 FROM dictionary_pointers d
                                    WHERE d.cache_id = s.cache_id AND d.segment_id = s.segment_id)
                   AND NOT EXISTS (SELECT 1 FROM entries e
                                    WHERE e.cache_id = s.cache_id AND e.dict_id = s.segment_id)
                 ORDER BY s.created_at LIMIT ?2",
            )
            .db()?;
        let rows = statement
            .query_map(params![created_before, int(limit as u64)], |row| {
                Ok((
                    CacheId::from_bytes(row.blob(0)?),
                    SegmentId::from_bytes(row.blob(1)?),
                ))
            })
            .db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }
}
