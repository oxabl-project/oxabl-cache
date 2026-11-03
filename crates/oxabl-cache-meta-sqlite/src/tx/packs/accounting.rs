//! Byte totals and the live-byte audit.

use crate::db::{DbResult, RowExt};
use crate::tx::SqliteTx;
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::namespace::NamespaceKey;
use oxabl_cache_core::domain::pack::PackTotals;
use oxabl_cache_core::ids::{CacheId, PackId};
use oxabl_cache_core::ports::metadata::PackAccountingTx;

impl PackAccountingTx for SqliteTx<'_> {
    fn pack_totals(&self, cache: &CacheId) -> Result<PackTotals> {
        let reclaimable: u64 = self
            .conn
            .prepare_cached(
                "SELECT
                   (SELECT COALESCE(SUM(size), 0) FROM packs
                     WHERE cache_id = ?1 AND (state = 2 OR (state = 1 AND dead_since IS NOT NULL)))
                 + (SELECT COALESCE(SUM(s.byte_length), 0)
                      FROM segments s
                      JOIN packs p ON p.cache_id = s.cache_id AND p.pack_id = s.pack_id
                     WHERE p.cache_id = ?1 AND p.state = 1 AND p.dead_since IS NULL
                       AND s.dead_since IS NOT NULL)",
            )
            .db()?
            .query_row([cache.as_bytes().as_slice()], |row| row.unsigned(0))
            .db()?;
        self.conn
            .prepare_cached(
                "SELECT COALESCE(SUM(size), 0),
                        COALESCE(SUM(CASE WHEN state = 0 THEN size ELSE 0 END), 0),
                        COALESCE(SUM(live_bytes), 0)
                 FROM packs WHERE cache_id = ?1",
            )
            .db()?
            .query_row([cache.as_bytes().as_slice()], |row| {
                Ok(PackTotals {
                    stored_bytes: row.unsigned(0)?,
                    pending_bytes: row.unsigned(1)?,
                    live_bytes: row.unsigned(2)?,
                    reclaimable_bytes: reclaimable,
                })
            })
            .db()
    }

    fn namespace_bytes(&self, cache: &CacheId, namespace: &NamespaceKey) -> Result<u64> {
        self.conn
            .prepare_cached(
                "SELECT COALESCE(SUM(CASE WHEN state = 0 THEN size ELSE live_bytes END), 0)
                 FROM packs WHERE cache_id = ?1 AND ns = ?2 AND state IN (0, 1)",
            )
            .db()?
            .query_row([cache.as_bytes().as_slice(), namespace.as_slice()], |row| {
                row.unsigned(0)
            })
            .db()
    }

    fn audit_live_bytes(&self, cache: &CacheId) -> Result<Vec<String>> {
        let id = cache.as_bytes().as_slice();
        let mut problems = Vec::new();
        let mut packs = self
            .conn
            .prepare_cached(
                "SELECT p.pack_id, p.live_bytes, COALESCE(SUM(s.byte_length), 0)
                 FROM packs p LEFT JOIN segments s
                   ON s.cache_id = p.cache_id AND s.pack_id = p.pack_id AND s.dead_since IS NULL
                 WHERE p.cache_id = ?1
                 GROUP BY p.pack_id, p.live_bytes
                 HAVING p.live_bytes <> COALESCE(SUM(s.byte_length), 0)",
            )
            .db()?;
        let mismatched = packs
            .query_map([id], |row| {
                Ok((
                    PackId::from_bytes(row.blob(0)?),
                    row.unsigned(1)?,
                    row.unsigned(2)?,
                ))
            })
            .db()?;
        for row in mismatched {
            let (pack, stored, actual) = row.db()?;
            problems.push(format!(
                "pack {pack} live_bytes {stored} but live segments sum to {actual}"
            ));
        }
        let (live, pending, segments, pending_packs) = self
            .conn
            .query_row(
                "SELECT c.live_bytes, c.pending_bytes,
                        (SELECT COALESCE(SUM(byte_length), 0) FROM segments
                          WHERE cache_id = c.cache_id AND dead_since IS NULL),
                        (SELECT COALESCE(SUM(size), 0) FROM packs
                          WHERE cache_id = c.cache_id AND state = 0)
                 FROM caches c WHERE c.cache_id = ?1",
                [id],
                |row| {
                    Ok((
                        row.unsigned(0)?,
                        row.unsigned(1)?,
                        row.unsigned(2)?,
                        row.unsigned(3)?,
                    ))
                },
            )
            .db()?;
        if live != segments {
            problems.push(format!(
                "cache live_bytes {live} but live segments sum to {segments}"
            ));
        }
        if pending != pending_packs {
            problems.push(format!(
                "cache pending_bytes {pending} but pending packs sum to {pending_packs}"
            ));
        }
        let stray: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM segments s JOIN packs p
                   ON p.cache_id = s.cache_id AND p.pack_id = s.pack_id
                 WHERE s.cache_id = ?1 AND s.dead_since IS NULL AND p.state <> 1",
                [id],
                |row| row.get(0),
            )
            .db()?;
        if stray > 0 {
            problems.push(format!(
                "{stray} live segments sit in packs that are not live"
            ));
        }
        let dangling: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM entries e JOIN segments s
                   ON s.cache_id = e.cache_id AND s.segment_id = e.segment_id
                 WHERE e.cache_id = ?1 AND s.dead_since IS NOT NULL",
                [id],
                |row| row.get(0),
            )
            .db()?;
        if dangling > 0 {
            problems.push(format!("{dangling} entries name a dead segment"));
        }
        Ok(problems)
    }
}
