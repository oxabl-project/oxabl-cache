//! Entry rows and their access days.

use super::SqliteTx;
use crate::db::{DbResult, RowExt, placeholders};
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::entry::{Entry, EntryHit, EntryRef};
use oxabl_cache_core::domain::namespace::NamespaceKey;
use oxabl_cache_core::ids::{CacheId, LookupId, PackId, SegmentId};
use oxabl_cache_core::ports::metadata::{EntriesTx, EvictionScope};
use rusqlite::{Row, params, params_from_iter};

const CHUNK: usize = 400;
const TRUSTED: [u8; 16] = [0; 16];
const REF_COLUMNS: &str = "e.ns, e.lookup_id, e.segment_id, e.dict_id, a.access_day";
const REF_FROM: &str = "FROM entry_access a JOIN entries e \
     ON e.cache_id = a.cache_id AND e.ns = a.ns AND e.lookup_id = a.lookup_id";

fn ref_of(row: &Row<'_>) -> rusqlite::Result<EntryRef> {
    Ok(EntryRef {
        namespace: row.blob(0)?,
        lookup_id: LookupId::from_bytes(row.blob(1)?),
        segment_id: SegmentId::from_bytes(row.blob(2)?),
        dict_id: row.opt_blob(3)?.map(SegmentId::from_bytes),
        access_day: row.get(4)?,
    })
}

impl SqliteTx<'_> {
    fn refs(
        &self,
        condition: &str,
        args: &[&dyn rusqlite::ToSql],
        order: &str,
        limit: usize,
    ) -> Result<Vec<EntryRef>> {
        let sql = format!(
            "SELECT {REF_COLUMNS} {REF_FROM} WHERE a.cache_id = ?1 AND {condition} {order} LIMIT {}",
            limit
        );
        let mut statement = self.conn.prepare_cached(&sql).db()?;
        let rows = statement.query_map(args, ref_of).db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }
}

impl EntriesTx for SqliteTx<'_> {
    fn insert_entry(&mut self, entry: &Entry, access_day: i64) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(
                "INSERT INTO entries
                   (cache_id, ns, lookup_id, segment_id, dict_id, epoch, record, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (cache_id, ns, lookup_id) DO NOTHING",
            )
            .db()?
            .execute(params![
                entry.cache.as_bytes().as_slice(),
                entry.namespace.as_slice(),
                entry.lookup_id.as_bytes().as_slice(),
                entry.segment_id.as_bytes().as_slice(),
                entry.dict_id.as_ref().map(|id| id.as_bytes().as_slice()),
                entry.epoch,
                entry.record,
                entry.created_at,
            ])
            .db()?;
        if changed == 1 {
            self.conn
                .prepare_cached(
                    "INSERT INTO entry_access (cache_id, ns, lookup_id, access_day)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .db()?
                .execute(params![
                    entry.cache.as_bytes().as_slice(),
                    entry.namespace.as_slice(),
                    entry.lookup_id.as_bytes().as_slice(),
                    access_day
                ])
                .db()?;
        }
        Ok(changed == 1)
    }

    fn lookup_entries(
        &self,
        cache: &CacheId,
        namespaces: &[NamespaceKey],
        ids: &[LookupId],
        with_record: bool,
    ) -> Result<Vec<EntryHit>> {
        let mut hits = Vec::new();
        if namespaces.is_empty() {
            return Ok(hits);
        }
        let record = if with_record { ", e.record" } else { "" };
        for chunk in ids.chunks(CHUNK) {
            let first_id = 2 + namespaces.len();
            let sql = format!(
                "SELECT e.lookup_id, e.ns, s.pack_id, s.byte_offset, s.byte_length, e.dict_id,
                        a.access_day{record}
                 FROM entries e
                 JOIN segments s ON s.cache_id = e.cache_id AND s.segment_id = e.segment_id
                 JOIN packs p ON p.cache_id = s.cache_id AND p.pack_id = s.pack_id
                 JOIN entry_access a
                   ON a.cache_id = e.cache_id AND a.ns = e.ns AND a.lookup_id = e.lookup_id
                 WHERE e.cache_id = ?1 AND e.ns IN ({}) AND e.lookup_id IN ({})
                   AND s.dead_since IS NULL AND p.state = 1",
                placeholders(2, namespaces.len()),
                placeholders(first_id, chunk.len()),
            );
            let mut statement = self.conn.prepare_cached(&sql).db()?;
            let values = std::iter::once(cache.as_bytes().as_slice())
                .chain(namespaces.iter().map(|ns| ns.as_slice()))
                .chain(chunk.iter().map(|id| id.as_bytes().as_slice()));
            let rows = statement
                .query_map(params_from_iter(values), |row| {
                    Ok(EntryHit {
                        lookup_id: LookupId::from_bytes(row.blob(0)?),
                        namespace: row.blob(1)?,
                        pack: PackId::from_bytes(row.blob(2)?),
                        offset: row.unsigned(3)?,
                        length: row.small(4)?,
                        dict_id: row.opt_blob(5)?.map(SegmentId::from_bytes),
                        access_day: row.get(6)?,
                        record: if with_record { Some(row.get(7)?) } else { None },
                    })
                })
                .db()?;
            for row in rows {
                hits.push(row.db()?);
            }
        }
        Ok(hits)
    }

    fn touch_entries(
        &mut self,
        cache: &CacheId,
        day: i64,
        keys: &[(NamespaceKey, LookupId)],
    ) -> Result<()> {
        let mut statement = self
            .conn
            .prepare_cached(
                "UPDATE entry_access SET access_day = ?1
                 WHERE cache_id = ?2 AND ns = ?3 AND lookup_id = ?4 AND access_day < ?1",
            )
            .db()?;
        for (namespace, id) in keys {
            statement
                .execute(params![
                    day,
                    cache.as_bytes().as_slice(),
                    namespace.as_slice(),
                    id.as_bytes().as_slice()
                ])
                .db()?;
        }
        Ok(())
    }

    fn oldest_entries(
        &self,
        cache: &CacheId,
        scope: EvictionScope,
        limit: usize,
    ) -> Result<Vec<EntryRef>> {
        let condition = match scope {
            EvictionScope::Personal => "e.ns <> ?2",
            EvictionScope::Trusted => "e.ns = ?2",
        };
        self.refs(
            condition,
            &[&cache.as_bytes().as_slice(), &TRUSTED.as_slice()],
            "ORDER BY a.access_day, e.created_at, e.lookup_id",
            limit,
        )
    }

    fn entries_idle_before(
        &self,
        cache: &CacheId,
        before_day: i64,
        limit: usize,
    ) -> Result<Vec<EntryRef>> {
        self.refs(
            "a.access_day < ?2",
            &[&cache.as_bytes().as_slice(), &before_day],
            "ORDER BY a.access_day",
            limit,
        )
    }

    fn entries_below_epoch(
        &self,
        cache: &CacheId,
        epoch: u32,
        limit: usize,
    ) -> Result<Vec<EntryRef>> {
        self.refs(
            "e.epoch < ?2",
            &[&cache.as_bytes().as_slice(), &epoch],
            "ORDER BY e.epoch",
            limit,
        )
    }

    fn entries_of_namespace(
        &self,
        cache: &CacheId,
        namespace: &NamespaceKey,
        limit: usize,
    ) -> Result<Vec<EntryRef>> {
        self.refs(
            "e.ns = ?2",
            &[&cache.as_bytes().as_slice(), &namespace.as_slice()],
            "ORDER BY e.lookup_id",
            limit,
        )
    }

    fn entries_naming_segment(
        &self,
        cache: &CacheId,
        segment: &SegmentId,
    ) -> Result<Vec<EntryRef>> {
        self.refs(
            "(e.segment_id = ?2 OR e.dict_id = ?2)",
            &[&cache.as_bytes().as_slice(), &segment.as_bytes().as_slice()],
            "ORDER BY e.lookup_id",
            usize::try_from(i64::MAX).unwrap_or(usize::MAX),
        )
    }

    fn dictionary_in_use(&self, cache: &CacheId, segment: &SegmentId) -> Result<bool> {
        self.conn
            .prepare_cached(
                "SELECT EXISTS (SELECT 1 FROM entries WHERE cache_id = ?1 AND dict_id = ?2)",
            )
            .db()?
            .query_row(
                [cache.as_bytes().as_slice(), segment.as_bytes().as_slice()],
                |row| row.get::<_, bool>(0),
            )
            .db()
    }

    fn delete_entry(
        &mut self,
        cache: &CacheId,
        namespace: &NamespaceKey,
        lookup_id: &LookupId,
    ) -> Result<bool> {
        let key = [
            cache.as_bytes().as_slice(),
            namespace.as_slice(),
            lookup_id.as_bytes().as_slice(),
        ];
        self.conn
            .prepare_cached(
                "DELETE FROM entry_access WHERE cache_id = ?1 AND ns = ?2 AND lookup_id = ?3",
            )
            .db()?
            .execute(key)
            .db()?;
        let deleted = self
            .conn
            .prepare_cached(
                "DELETE FROM entries WHERE cache_id = ?1 AND ns = ?2 AND lookup_id = ?3",
            )
            .db()?
            .execute(key)
            .db()?;
        Ok(deleted == 1)
    }
}
