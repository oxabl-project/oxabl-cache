//! Pack selection for the sweep and compaction.

use super::mapping::{PACK_COLUMNS, collect, pack_of};
use crate::db::{DbResult, int};
use crate::tx::SqliteTx;
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::pack::Pack;
use oxabl_cache_core::ports::metadata::PackSelectionTx;
use rusqlite::params;

impl PackSelectionTx for SqliteTx<'_> {
    fn packs_due_for_sweep(&self, now: i64, grace: i64, limit: usize) -> Result<Vec<Pack>> {
        let mut statement = self
            .conn
            .prepare_cached(&format!(
                "SELECT {PACK_COLUMNS} FROM packs
                 WHERE (state = 1 AND dead_since IS NOT NULL AND dead_since <= ?1)
                    OR (state = 0 AND lease_until < ?2)
                    OR state = 2
                 ORDER BY created_at LIMIT ?3"
            ))
            .db()?;
        collect(
            statement
                .query_map(params![now - grace, now, int(limit as u64)], pack_of)
                .db()?,
        )
    }

    fn compaction_candidates(
        &self,
        created_before: i64,
        max_live_permille: u64,
        min_size: u64,
        limit: usize,
    ) -> Result<Vec<Pack>> {
        let mut statement = self
            .conn
            .prepare_cached(&format!(
                "SELECT {PACK_COLUMNS} FROM packs
                 WHERE state = 1 AND dead_since IS NULL AND live_bytes > 0
                   AND created_at <= ?1 AND size >= ?2 AND live_bytes * 1000 < size * ?3
                 ORDER BY created_at LIMIT ?4"
            ))
            .db()?;
        collect(
            statement
                .query_map(
                    params![
                        created_before,
                        int(min_size),
                        int(max_live_permille),
                        int(limit as u64)
                    ],
                    pack_of,
                )
                .db()?,
        )
    }

    fn empty_unmarked_packs(&self, limit: usize) -> Result<Vec<Pack>> {
        let mut statement = self
            .conn
            .prepare_cached(&format!(
                "SELECT {PACK_COLUMNS} FROM packs
                 WHERE state = 1 AND live_bytes = 0 AND dead_since IS NULL LIMIT ?1"
            ))
            .db()?;
        collect(statement.query_map([int(limit as u64)], pack_of).db()?)
    }
}
