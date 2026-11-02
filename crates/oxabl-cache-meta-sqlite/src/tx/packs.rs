//! Pack rows, their accounting, the scrub queue.

use super::SqliteTx;
use crate::db::{DbResult, RowExt, int};
use oxabl_cache_core::domain::namespace::NamespaceKey;
use oxabl_cache_core::domain::pack::{Pack, PackState, PackTotals};
use oxabl_cache_core::ids::{CacheId, ClientRef, PackId, PrincipalId};
use oxabl_cache_core::ports::metadata::{PacksTx, RecordedCommit};
use oxabl_cache_core::{Error, Result};
use rusqlite::{OptionalExtension, Row, params};

const PACK_COLUMNS: &str = "cache_id, pack_id, ns, uploader, client_ref, size, etag, live_bytes, \
     state, lease_until, created_at, dead_since, commit_digest, commit_statuses";

fn state_code(state: PackState) -> i64 {
    match state {
        PackState::Pending => 0,
        PackState::Live => 1,
        PackState::Deleting => 2,
    }
}

fn state_of(code: i64) -> Option<PackState> {
    match code {
        0 => Some(PackState::Pending),
        1 => Some(PackState::Live),
        2 => Some(PackState::Deleting),
        _ => None,
    }
}

fn pack_of(row: &Row<'_>) -> rusqlite::Result<Pack> {
    let state: i64 = row.get(8)?;
    Ok(Pack {
        cache: CacheId::from_bytes(row.blob(0)?),
        id: PackId::from_bytes(row.blob(1)?),
        namespace: row.blob(2)?,
        uploader: PrincipalId::from_bytes(row.blob(3)?),
        client_ref: ClientRef::from_bytes(row.blob(4)?),
        size: row.unsigned(5)?,
        etag: row.get(6)?,
        live_bytes: row.unsigned(7)?,
        state: state_of(state).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                8,
                rusqlite::types::Type::Integer,
                "stored pack state is unknown".into(),
            )
        })?,
        lease_until: row.get(9)?,
        created_at: row.get(10)?,
        dead_since: row.get(11)?,
        commit_digest: row.opt_blob(12)?,
        commit_statuses: row.get(13)?,
    })
}

fn collect(
    rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<Pack>>,
) -> Result<Vec<Pack>> {
    rows.collect::<rusqlite::Result<_>>().db()
}

impl PacksTx for SqliteTx<'_> {
    fn pack(&self, cache: &CacheId, id: &PackId) -> Result<Option<Pack>> {
        self.conn
            .prepare_cached(&format!(
                "SELECT {PACK_COLUMNS} FROM packs WHERE cache_id = ?1 AND pack_id = ?2"
            ))
            .db()?
            .query_row(
                [cache.as_bytes().as_slice(), id.as_bytes().as_slice()],
                pack_of,
            )
            .optional()
            .db()
    }

    fn pack_by_client_ref(
        &self,
        cache: &CacheId,
        uploader: &PrincipalId,
        client_ref: &ClientRef,
    ) -> Result<Option<Pack>> {
        self.conn
            .prepare_cached(&format!(
                "SELECT {PACK_COLUMNS} FROM packs
                 WHERE cache_id = ?1 AND uploader = ?2 AND client_ref = ?3"
            ))
            .db()?
            .query_row(
                [
                    cache.as_bytes().as_slice(),
                    uploader.as_bytes().as_slice(),
                    client_ref.as_bytes().as_slice(),
                ],
                pack_of,
            )
            .optional()
            .db()
    }

    fn insert_pack(&mut self, pack: &Pack) -> Result<()> {
        self.conn
            .prepare_cached(&format!(
                "INSERT INTO packs ({PACK_COLUMNS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"
            ))
            .db()?
            .execute(params![
                pack.cache.as_bytes().as_slice(),
                pack.id.as_bytes().as_slice(),
                pack.namespace.as_slice(),
                pack.uploader.as_bytes().as_slice(),
                pack.client_ref.as_bytes().as_slice(),
                int(pack.size),
                pack.etag,
                int(pack.live_bytes),
                state_code(pack.state),
                pack.lease_until,
                pack.created_at,
                pack.dead_since,
                pack.commit_digest.as_ref().map(<[u8; 32]>::as_slice),
                pack.commit_statuses,
            ])
            .db()?;
        Ok(())
    }

    fn pack_totals(&self, cache: &CacheId) -> Result<PackTotals> {
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

    fn record_commit(
        &mut self,
        cache: &CacheId,
        id: &PackId,
        commit: &RecordedCommit<'_>,
    ) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(
                "UPDATE packs SET state = 1, etag = ?3, live_bytes = ?4, commit_digest = ?5,
                                  commit_statuses = ?6, dead_since = ?7
                 WHERE cache_id = ?1 AND pack_id = ?2 AND state = 0 AND commit_digest IS NULL",
            )
            .db()?
            .execute(params![
                cache.as_bytes().as_slice(),
                id.as_bytes().as_slice(),
                commit.etag,
                int(commit.live_bytes),
                commit.digest.as_slice(),
                commit.statuses,
                commit.dead_since,
            ])
            .db()?;
        Ok(changed == 1)
    }

    fn set_pack_etag(&mut self, cache: &CacheId, id: &PackId, etag: &str) -> Result<()> {
        self.conn
            .prepare_cached(
                "UPDATE packs SET etag = ?3 WHERE cache_id = ?1 AND pack_id = ?2 AND state = 1",
            )
            .db()?
            .execute(params![
                cache.as_bytes().as_slice(),
                id.as_bytes().as_slice(),
                etag
            ])
            .db()?;
        Ok(())
    }

    fn adjust_pack_live(&mut self, cache: &CacheId, id: &PackId, delta: i64) -> Result<u64> {
        let changed = self
            .conn
            .prepare_cached(
                "UPDATE packs SET live_bytes = live_bytes + ?3 WHERE cache_id = ?1 AND pack_id = ?2",
            )
            .db()?
            .execute(params![cache.as_bytes().as_slice(), id.as_bytes().as_slice(), delta])
            .db()?;
        if changed != 1 {
            return Err(Error::internal("live bytes adjusted for an unknown pack"));
        }
        self.conn
            .prepare_cached("SELECT live_bytes FROM packs WHERE cache_id = ?1 AND pack_id = ?2")
            .db()?
            .query_row(
                [cache.as_bytes().as_slice(), id.as_bytes().as_slice()],
                |row| row.unsigned(0),
            )
            .db()
    }

    fn mark_pack_dead(&mut self, cache: &CacheId, id: &PackId, now: i64) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(
                "UPDATE packs SET dead_since = ?3
                 WHERE cache_id = ?1 AND pack_id = ?2 AND state = 1 AND dead_since IS NULL",
            )
            .db()?
            .execute(params![
                cache.as_bytes().as_slice(),
                id.as_bytes().as_slice(),
                now
            ])
            .db()?;
        Ok(changed == 1)
    }

    fn set_pack_deleting(&mut self, cache: &CacheId, id: &PackId) -> Result<Option<PackState>> {
        let Some(previous) = self.pack(cache, id)?.map(|pack| pack.state) else {
            return Ok(None);
        };
        if previous == PackState::Deleting {
            return Ok(None);
        }
        self.conn
            .prepare_cached(
                "UPDATE packs SET state = 2 WHERE cache_id = ?1 AND pack_id = ?2 AND state IN (0, 1)",
            )
            .db()?
            .execute([cache.as_bytes().as_slice(), id.as_bytes().as_slice()])
            .db()?;
        Ok(Some(previous))
    }

    fn delete_pack(&mut self, cache: &CacheId, id: &PackId) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached("DELETE FROM packs WHERE cache_id = ?1 AND pack_id = ?2 AND state = 2")
            .db()?
            .execute([cache.as_bytes().as_slice(), id.as_bytes().as_slice()])
            .db()?;
        Ok(changed == 1)
    }

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

    fn pack_ids(&self, cache: &CacheId) -> Result<Vec<PackId>> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT pack_id FROM packs WHERE cache_id = ?1")
            .db()?;
        let rows = statement
            .query_map([cache.as_bytes().as_slice()], |row| {
                Ok(PackId::from_bytes(row.blob(0)?))
            })
            .db()?;
        rows.collect::<rusqlite::Result<_>>().db()
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

    fn queue_scrub(&mut self, cache: &CacheId, id: &PackId, now: i64) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO scrub_queue (cache_id, pack_id, queued_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (cache_id, pack_id) DO NOTHING",
            )
            .db()?
            .execute(params![
                cache.as_bytes().as_slice(),
                id.as_bytes().as_slice(),
                now
            ])
            .db()?;
        Ok(())
    }

    fn take_scrubs(&mut self, limit: usize) -> Result<Vec<(CacheId, PackId)>> {
        let queued: Vec<(CacheId, PackId)> = {
            let mut statement = self
                .conn
                .prepare_cached(
                    "SELECT cache_id, pack_id FROM scrub_queue ORDER BY queued_at LIMIT ?1",
                )
                .db()?;
            let rows = statement
                .query_map([int(limit as u64)], |row| {
                    Ok((
                        CacheId::from_bytes(row.blob(0)?),
                        PackId::from_bytes(row.blob(1)?),
                    ))
                })
                .db()?;
            rows.collect::<rusqlite::Result<_>>().db()?
        };
        for (cache, pack) in &queued {
            self.conn
                .execute(
                    "DELETE FROM scrub_queue WHERE cache_id = ?1 AND pack_id = ?2",
                    [cache.as_bytes().as_slice(), pack.as_bytes().as_slice()],
                )
                .db()?;
        }
        Ok(queued)
    }
}
