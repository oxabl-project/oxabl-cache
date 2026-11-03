//! Entry rows and their access days.

use super::{PgTx, RowExt, args, byte_array};
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::entry::{Entry, EntryHit, EntryRef};
use oxabl_cache_core::domain::namespace::NamespaceKey;
use oxabl_cache_core::ids::{CacheId, LookupId, PackId, SegmentId};
use oxabl_cache_core::ports::metadata::{EntriesTx, EvictionScope};
use std::collections::HashSet;
use tokio_postgres::Row;

const TRUSTED: [u8; 16] = [0; 16];
const REF_COLUMNS: &str = "e.ns, e.lookup_id, e.segment_id, e.dict_id, a.access_day";
const REF_FROM: &str = "FROM entry_access a JOIN entries e \
     ON e.cache_id = a.cache_id AND e.ns = a.ns AND e.lookup_id = a.lookup_id";

fn ref_of(row: &Row) -> Result<EntryRef> {
    Ok(EntryRef {
        namespace: row.blob(0)?,
        lookup_id: LookupId::from_bytes(row.blob(1)?),
        segment_id: SegmentId::from_bytes(row.blob(2)?),
        dict_id: row.opt_blob(3)?.map(SegmentId::from_bytes),
        access_day: row.int(4)?,
    })
}

impl PgTx<'_> {
    fn refs(
        &self,
        condition: &str,
        args: super::Args<'_>,
        order: &str,
        limit: usize,
    ) -> Result<Vec<EntryRef>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let sql = format!(
            "SELECT {REF_COLUMNS} {REF_FROM} WHERE a.cache_id = $1 AND {condition} {order} LIMIT {limit}"
        );
        self.query(&sql, args)?.iter().map(ref_of).collect()
    }
}

impl EntriesTx for PgTx<'_> {
    fn insert_entry(&mut self, entry: &Entry, access_day: i64) -> Result<bool> {
        let changed = self.execute(
            "INSERT INTO entries
               (cache_id, ns, lookup_id, segment_id, dict_id, epoch, record, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (cache_id, ns, lookup_id) DO NOTHING",
            args![
                entry.cache.as_bytes().as_slice(),
                entry.namespace.as_slice(),
                entry.lookup_id.as_bytes().as_slice(),
                entry.segment_id.as_bytes().as_slice(),
                entry.dict_id.as_ref().map(|id| id.as_bytes().as_slice()),
                i64::from(entry.epoch),
                entry.record,
                entry.created_at,
            ],
        )?;
        if changed == 1 {
            self.execute(
                "INSERT INTO entry_access (cache_id, ns, lookup_id, access_day)
                 VALUES ($1, $2, $3, $4)",
                args![
                    entry.cache.as_bytes().as_slice(),
                    entry.namespace.as_slice(),
                    entry.lookup_id.as_bytes().as_slice(),
                    access_day
                ],
            )?;
        }
        Ok(changed == 1)
    }

    fn insert_entries(&mut self, entries: &[Entry], access_day: i64) -> Result<Vec<bool>> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        let bytes = |pick: fn(&Entry) -> &[u8]| byte_array(entries.iter().map(pick));
        let cache = bytes(|e| e.cache.as_bytes().as_slice());
        let namespace = bytes(|e| e.namespace.as_slice());
        let lookup = bytes(|e| e.lookup_id.as_bytes().as_slice());
        let segment = bytes(|e| e.segment_id.as_bytes().as_slice());
        let dict: Vec<Option<Vec<u8>>> = entries
            .iter()
            .map(|e| e.dict_id.map(|id| id.as_bytes().to_vec()))
            .collect();
        let epoch: Vec<i64> = entries.iter().map(|e| i64::from(e.epoch)).collect();
        let record = bytes(|e| e.record.as_slice());
        let created: Vec<i64> = entries.iter().map(|e| e.created_at).collect();
        let won: HashSet<([u8; 16], [u8; 32])> = self
            .query(
                "WITH wanted AS (
                   SELECT * FROM unnest($1::bytea[], $2::bytea[], $3::bytea[], $4::bytea[],
                                        $5::bytea[], $6::bigint[], $7::bytea[], $8::bigint[])
                        WITH ORDINALITY AS t (cache_id, ns, lookup_id, segment_id, dict_id,
                                              epoch, record, created_at, position)
                 ), inserted AS (
                   INSERT INTO entries
                     (cache_id, ns, lookup_id, segment_id, dict_id, epoch, record, created_at)
                   SELECT cache_id, ns, lookup_id, segment_id, dict_id, epoch, record, created_at
                   FROM wanted ORDER BY position
                   ON CONFLICT (cache_id, ns, lookup_id) DO NOTHING
                   RETURNING cache_id, ns, lookup_id
                 ), touched AS (
                   INSERT INTO entry_access (cache_id, ns, lookup_id, access_day)
                   SELECT cache_id, ns, lookup_id, $9 FROM inserted
                 )
                 SELECT ns, lookup_id FROM inserted",
                args![
                    cache, namespace, lookup, segment, dict, epoch, record, created, access_day
                ],
            )?
            .iter()
            .map(|row| Ok((row.blob(0)?, row.blob(1)?)))
            .collect::<Result<_>>()?;
        let mut seen = HashSet::new();
        Ok(entries
            .iter()
            .map(|entry| {
                let key = (entry.namespace, *entry.lookup_id.as_bytes());
                won.contains(&key) && seen.insert(key)
            })
            .collect())
    }

    fn lookup_entries(
        &self,
        cache: &CacheId,
        namespaces: &[NamespaceKey],
        ids: &[LookupId],
        with_record: bool,
    ) -> Result<Vec<EntryHit>> {
        if namespaces.is_empty() || ids.is_empty() {
            return Ok(Vec::new());
        }
        let record = if with_record { ", e.record" } else { "" };
        let sql = format!(
            "SELECT e.lookup_id, e.ns, s.pack_id, s.byte_offset, s.byte_length, e.dict_id,
                    a.access_day{record}
             FROM entries e
             JOIN segments s ON s.cache_id = e.cache_id AND s.segment_id = e.segment_id
             JOIN packs p ON p.cache_id = s.cache_id AND p.pack_id = s.pack_id
             JOIN entry_access a
               ON a.cache_id = e.cache_id AND a.ns = e.ns AND a.lookup_id = e.lookup_id
             WHERE e.cache_id = $1 AND e.ns = ANY($2) AND e.lookup_id = ANY($3)
               AND s.dead_since IS NULL AND p.state = 1"
        );
        let namespaces = byte_array(namespaces.iter().map(|ns| ns.as_slice()));
        let ids = byte_array(ids.iter().map(|id| id.as_bytes().as_slice()));
        self.query(&sql, args![cache.as_bytes().as_slice(), namespaces, ids])?
            .iter()
            .map(|row| {
                Ok(EntryHit {
                    lookup_id: LookupId::from_bytes(row.blob(0)?),
                    namespace: row.blob(1)?,
                    pack: PackId::from_bytes(row.blob(2)?),
                    offset: row.unsigned(3)?,
                    length: row.small(4)?,
                    dict_id: row.opt_blob(5)?.map(SegmentId::from_bytes),
                    access_day: row.int(6)?,
                    record: if with_record {
                        Some(row.bytes(7)?)
                    } else {
                        None
                    },
                })
            })
            .collect()
    }

    fn touch_entries(
        &mut self,
        cache: &CacheId,
        day: i64,
        keys: &[(NamespaceKey, LookupId)],
    ) -> Result<()> {
        if keys.is_empty() {
            return Ok(());
        }
        let namespaces = byte_array(keys.iter().map(|(ns, _)| ns.as_slice()));
        let ids = byte_array(keys.iter().map(|(_, id)| id.as_bytes().as_slice()));
        self.execute(
            "UPDATE entry_access a SET access_day = $1
             FROM unnest($3::bytea[], $4::bytea[]) AS k (ns, lookup_id)
             WHERE a.cache_id = $2 AND a.ns = k.ns AND a.lookup_id = k.lookup_id
               AND a.access_day < $1",
            args![day, cache.as_bytes().as_slice(), namespaces, ids],
        )?;
        Ok(())
    }

    fn oldest_entries(
        &self,
        cache: &CacheId,
        scope: EvictionScope,
        limit: usize,
    ) -> Result<Vec<EntryRef>> {
        let condition = match scope {
            EvictionScope::Personal => "e.ns <> $2",
            EvictionScope::Trusted => "e.ns = $2",
        };
        self.refs(
            condition,
            args![cache.as_bytes().as_slice(), TRUSTED.as_slice()],
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
            "a.access_day < $2",
            args![cache.as_bytes().as_slice(), before_day],
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
            "e.epoch < $2",
            args![cache.as_bytes().as_slice(), i64::from(epoch)],
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
            "e.ns = $2",
            args![cache.as_bytes().as_slice(), namespace.as_slice()],
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
            "(e.segment_id = $2 OR e.dict_id = $2)",
            args![cache.as_bytes().as_slice(), segment.as_bytes().as_slice()],
            "ORDER BY e.lookup_id",
            usize::MAX,
        )
    }

    fn dictionary_in_use(&self, cache: &CacheId, segment: &SegmentId) -> Result<bool> {
        self.query_one(
            "SELECT EXISTS (SELECT 1 FROM entries WHERE cache_id = $1 AND dict_id = $2)",
            args![cache.as_bytes().as_slice(), segment.as_bytes().as_slice()],
        )?
        .flag(0)
    }

    fn delete_entry(
        &mut self,
        cache: &CacheId,
        namespace: &NamespaceKey,
        lookup_id: &LookupId,
    ) -> Result<bool> {
        self.execute(
            "DELETE FROM entry_access WHERE cache_id = $1 AND ns = $2 AND lookup_id = $3",
            args![
                cache.as_bytes().as_slice(),
                namespace.as_slice(),
                lookup_id.as_bytes().as_slice()
            ],
        )?;
        let deleted = self.execute(
            "DELETE FROM entries WHERE cache_id = $1 AND ns = $2 AND lookup_id = $3",
            args![
                cache.as_bytes().as_slice(),
                namespace.as_slice(),
                lookup_id.as_bytes().as_slice()
            ],
        )?;
        Ok(deleted == 1)
    }
}
