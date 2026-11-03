//! Pack rows, their accounting, the scrub queue.

use super::{PgTx, RowExt, args, int};
use oxabl_cache_core::domain::namespace::NamespaceKey;
use oxabl_cache_core::domain::pack::{Pack, PackState, PackTotals};
use oxabl_cache_core::ids::{CacheId, ClientRef, PackId, PrincipalId};
use oxabl_cache_core::ports::metadata::{PacksTx, RecordedCommit};
use oxabl_cache_core::{Error, Result};
use tokio_postgres::Row;

const PACK_COLUMNS: &str = "cache_id, pack_id, ns, uploader, client_ref, size, etag, live_bytes, \
     state, lease_until, created_at, dead_since, commit_digest, commit_statuses";

fn state_code(state: PackState) -> i64 {
    match state {
        PackState::Pending => 0,
        PackState::Live => 1,
        PackState::Deleting => 2,
    }
}

fn state_of(code: i64) -> Result<PackState> {
    match code {
        0 => Ok(PackState::Pending),
        1 => Ok(PackState::Live),
        2 => Ok(PackState::Deleting),
        _ => Err(Error::internal("stored pack state is unknown")),
    }
}

fn pack_of(row: &Row) -> Result<Pack> {
    Ok(Pack {
        cache: CacheId::from_bytes(row.blob(0)?),
        id: PackId::from_bytes(row.blob(1)?),
        namespace: row.blob(2)?,
        uploader: PrincipalId::from_bytes(row.blob(3)?),
        client_ref: ClientRef::from_bytes(row.blob(4)?),
        size: row.unsigned(5)?,
        etag: row.opt_text(6)?,
        live_bytes: row.unsigned(7)?,
        state: state_of(row.int(8)?)?,
        lease_until: row.int(9)?,
        created_at: row.int(10)?,
        dead_since: row.opt_int(11)?,
        commit_digest: row.opt_blob(12)?,
        commit_statuses: row.opt_bytes(13)?,
    })
}

fn packs(rows: &[Row]) -> Result<Vec<Pack>> {
    rows.iter().map(pack_of).collect()
}

impl PacksTx for PgTx<'_> {
    fn pack(&self, cache: &CacheId, id: &PackId) -> Result<Option<Pack>> {
        self.query_opt(
            &format!("SELECT {PACK_COLUMNS} FROM packs WHERE cache_id = $1 AND pack_id = $2"),
            args![cache.as_bytes().as_slice(), id.as_bytes().as_slice()],
        )?
        .as_ref()
        .map(pack_of)
        .transpose()
    }

    fn pack_by_client_ref(
        &self,
        cache: &CacheId,
        uploader: &PrincipalId,
        client_ref: &ClientRef,
    ) -> Result<Option<Pack>> {
        self.query_opt(
            &format!(
                "SELECT {PACK_COLUMNS} FROM packs
                 WHERE cache_id = $1 AND uploader = $2 AND client_ref = $3"
            ),
            args![
                cache.as_bytes().as_slice(),
                uploader.as_bytes().as_slice(),
                client_ref.as_bytes().as_slice()
            ],
        )?
        .as_ref()
        .map(pack_of)
        .transpose()
    }

    fn insert_pack(&mut self, pack: &Pack) -> Result<()> {
        self.execute(
            &format!(
                "INSERT INTO packs ({PACK_COLUMNS})
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)"
            ),
            args![
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
            ],
        )?;
        Ok(())
    }

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

    fn record_commit(
        &mut self,
        cache: &CacheId,
        id: &PackId,
        commit: &RecordedCommit<'_>,
    ) -> Result<bool> {
        let changed = self.execute(
            "UPDATE packs SET state = 1, etag = $3, live_bytes = $4, commit_digest = $5,
                              commit_statuses = $6, dead_since = $7
             WHERE cache_id = $1 AND pack_id = $2 AND state = 0 AND commit_digest IS NULL",
            args![
                cache.as_bytes().as_slice(),
                id.as_bytes().as_slice(),
                commit.etag,
                int(commit.live_bytes),
                commit.digest.as_slice(),
                commit.statuses,
                commit.dead_since,
            ],
        )?;
        Ok(changed == 1)
    }

    fn set_pack_etag(&mut self, cache: &CacheId, id: &PackId, etag: &str) -> Result<()> {
        self.execute(
            "UPDATE packs SET etag = $3 WHERE cache_id = $1 AND pack_id = $2 AND state = 1",
            args![cache.as_bytes().as_slice(), id.as_bytes().as_slice(), etag],
        )?;
        Ok(())
    }

    fn adjust_pack_live(&mut self, cache: &CacheId, id: &PackId, delta: i64) -> Result<u64> {
        self.query_opt(
            "UPDATE packs SET live_bytes = live_bytes + $3
             WHERE cache_id = $1 AND pack_id = $2 RETURNING live_bytes",
            args![cache.as_bytes().as_slice(), id.as_bytes().as_slice(), delta],
        )?
        .ok_or_else(|| Error::internal("live bytes adjusted for an unknown pack"))?
        .unsigned(0)
    }

    fn mark_pack_dead(&mut self, cache: &CacheId, id: &PackId, now: i64) -> Result<bool> {
        let changed = self.execute(
            "UPDATE packs SET dead_since = $3
             WHERE cache_id = $1 AND pack_id = $2 AND state = 1 AND dead_since IS NULL",
            args![cache.as_bytes().as_slice(), id.as_bytes().as_slice(), now],
        )?;
        Ok(changed == 1)
    }

    fn set_pack_deleting(&mut self, cache: &CacheId, id: &PackId) -> Result<Option<PackState>> {
        let Some(previous) = self.pack(cache, id)?.map(|pack| pack.state) else {
            return Ok(None);
        };
        if previous == PackState::Deleting {
            return Ok(None);
        }
        self.execute(
            "UPDATE packs SET state = 2 WHERE cache_id = $1 AND pack_id = $2 AND state IN (0, 1)",
            args![cache.as_bytes().as_slice(), id.as_bytes().as_slice()],
        )?;
        Ok(Some(previous))
    }

    fn delete_pack(&mut self, cache: &CacheId, id: &PackId) -> Result<bool> {
        let changed = self.execute(
            "DELETE FROM packs WHERE cache_id = $1 AND pack_id = $2 AND state = 2",
            args![cache.as_bytes().as_slice(), id.as_bytes().as_slice()],
        )?;
        Ok(changed == 1)
    }

    fn packs_due_for_sweep(&self, now: i64, grace: i64, limit: usize) -> Result<Vec<Pack>> {
        packs(&self.query(
            &format!(
                "SELECT {PACK_COLUMNS} FROM packs
                 WHERE (state = 1 AND dead_since IS NOT NULL AND dead_since <= $1)
                    OR (state = 0 AND lease_until < $2)
                    OR state = 2
                 ORDER BY created_at LIMIT $3"
            ),
            args![now - grace, now, int(limit as u64)],
        )?)
    }

    fn compaction_candidates(
        &self,
        created_before: i64,
        max_live_permille: u64,
        min_size: u64,
        limit: usize,
    ) -> Result<Vec<Pack>> {
        packs(&self.query(
            &format!(
                "SELECT {PACK_COLUMNS} FROM packs
                 WHERE state = 1 AND dead_since IS NULL AND live_bytes > 0
                   AND created_at <= $1 AND size >= $2 AND live_bytes * 1000 < size * $3
                 ORDER BY created_at LIMIT $4"
            ),
            args![
                created_before,
                int(min_size),
                int(max_live_permille),
                int(limit as u64)
            ],
        )?)
    }

    fn empty_unmarked_packs(&self, limit: usize) -> Result<Vec<Pack>> {
        packs(&self.query(
            &format!(
                "SELECT {PACK_COLUMNS} FROM packs
                 WHERE state = 1 AND live_bytes = 0 AND dead_since IS NULL LIMIT $1"
            ),
            args![int(limit as u64)],
        )?)
    }

    fn pack_ids(&self, cache: &CacheId) -> Result<Vec<PackId>> {
        self.query(
            "SELECT pack_id FROM packs WHERE cache_id = $1",
            args![cache.as_bytes().as_slice()],
        )?
        .iter()
        .map(|row| Ok(PackId::from_bytes(row.blob(0)?)))
        .collect()
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

    fn queue_scrub(&mut self, cache: &CacheId, id: &PackId, now: i64) -> Result<()> {
        self.execute(
            "INSERT INTO scrub_queue (cache_id, pack_id, queued_at) VALUES ($1, $2, $3)
             ON CONFLICT (cache_id, pack_id) DO NOTHING",
            args![cache.as_bytes().as_slice(), id.as_bytes().as_slice(), now],
        )?;
        Ok(())
    }

    fn take_scrubs(&mut self, limit: usize) -> Result<Vec<(CacheId, PackId)>> {
        self.query(
            "DELETE FROM scrub_queue WHERE (cache_id, pack_id) IN
               (SELECT cache_id, pack_id FROM scrub_queue ORDER BY queued_at LIMIT $1)
             RETURNING cache_id, pack_id",
            args![int(limit as u64)],
        )?
        .iter()
        .map(|row| {
            Ok((
                CacheId::from_bytes(row.blob(0)?),
                PackId::from_bytes(row.blob(1)?),
            ))
        })
        .collect()
    }
}
