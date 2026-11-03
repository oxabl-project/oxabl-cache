//! Byte totals and the live-byte audit.

use crate::tx::{PgTx, RowExt, args};
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::namespace::NamespaceKey;
use oxabl_cache_core::domain::pack::PackTotals;
use oxabl_cache_core::ids::{CacheId, PackId};
use oxabl_cache_core::ports::metadata::PackAccountingTx;

impl PackAccountingTx for PgTx<'_> {
    fn pack_totals(&self, cache: &CacheId) -> Result<PackTotals> {
        let reclaimable = self
            .query_one(
                "SELECT ((SELECT COALESCE(SUM(size), 0) FROM packs
                     WHERE cache_id = $1 AND (state = 2 OR (state = 1 AND dead_since IS NOT NULL)))
                 + (SELECT COALESCE(SUM(s.byte_length), 0)
                      FROM segments s
                      JOIN packs p ON p.cache_id = s.cache_id AND p.pack_id = s.pack_id
                     WHERE p.cache_id = $1 AND p.state = 1 AND p.dead_since IS NULL
                       AND s.dead_since IS NOT NULL))::bigint",
                args![cache.as_bytes().as_slice()],
            )?
            .unsigned(0)?;
        let row = self.query_one(
            "SELECT COALESCE(SUM(size), 0)::bigint,
                    COALESCE(SUM(CASE WHEN state = 0 THEN size ELSE 0 END), 0)::bigint,
                    COALESCE(SUM(live_bytes), 0)::bigint
             FROM packs WHERE cache_id = $1",
            args![cache.as_bytes().as_slice()],
        )?;
        Ok(PackTotals {
            stored_bytes: row.unsigned(0)?,
            pending_bytes: row.unsigned(1)?,
            live_bytes: row.unsigned(2)?,
            reclaimable_bytes: reclaimable,
        })
    }

    fn namespace_bytes(&self, cache: &CacheId, namespace: &NamespaceKey) -> Result<u64> {
        self.query_one(
            "SELECT COALESCE(SUM(CASE WHEN state = 0 THEN size ELSE live_bytes END), 0)::bigint
             FROM packs WHERE cache_id = $1 AND ns = $2 AND state IN (0, 1)",
            args![cache.as_bytes().as_slice(), namespace.as_slice()],
        )?
        .unsigned(0)
    }

    fn audit_live_bytes(&self, cache: &CacheId) -> Result<Vec<String>> {
        let id = cache.as_bytes().as_slice();
        let mut problems = Vec::new();
        for row in self.query(
            "SELECT p.pack_id, p.live_bytes, COALESCE(SUM(s.byte_length), 0)::bigint
             FROM packs p LEFT JOIN segments s
               ON s.cache_id = p.cache_id AND s.pack_id = p.pack_id AND s.dead_since IS NULL
             WHERE p.cache_id = $1
             GROUP BY p.pack_id, p.live_bytes
             HAVING p.live_bytes <> COALESCE(SUM(s.byte_length), 0)",
            args![id],
        )? {
            let (pack, stored, actual) = (
                PackId::from_bytes(row.blob(0)?),
                row.unsigned(1)?,
                row.unsigned(2)?,
            );
            problems.push(format!(
                "pack {pack} live_bytes {stored} but live segments sum to {actual}"
            ));
        }
        let totals = self.query_one(
            "SELECT c.live_bytes, c.pending_bytes,
                    (SELECT COALESCE(SUM(byte_length), 0)::bigint FROM segments
                      WHERE cache_id = c.cache_id AND dead_since IS NULL),
                    (SELECT COALESCE(SUM(size), 0)::bigint FROM packs
                      WHERE cache_id = c.cache_id AND state = 0)
             FROM caches c WHERE c.cache_id = $1",
            args![id],
        )?;
        let (live, pending, segments, pending_packs) = (
            totals.unsigned(0)?,
            totals.unsigned(1)?,
            totals.unsigned(2)?,
            totals.unsigned(3)?,
        );
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
        let stray = self
            .query_one(
                "SELECT COUNT(*) FROM segments s JOIN packs p
                   ON p.cache_id = s.cache_id AND p.pack_id = s.pack_id
                 WHERE s.cache_id = $1 AND s.dead_since IS NULL AND p.state <> 1",
                args![id],
            )?
            .int(0)?;
        if stray > 0 {
            problems.push(format!(
                "{stray} live segments sit in packs that are not live"
            ));
        }
        let dangling = self
            .query_one(
                "SELECT COUNT(*) FROM entries e JOIN segments s
                   ON s.cache_id = e.cache_id AND s.segment_id = e.segment_id
                 WHERE e.cache_id = $1 AND s.dead_since IS NOT NULL",
                args![id],
            )?
            .int(0)?;
        if dangling > 0 {
            problems.push(format!("{dangling} entries name a dead segment"));
        }
        Ok(problems)
    }
}
