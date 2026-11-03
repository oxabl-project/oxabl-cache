//! Pack rows and their lifecycle.

use super::mapping::{PACK_COLUMNS, pack_of, state_code};
use crate::db::{DbResult, RowExt, int};
use crate::tx::SqliteTx;
use oxabl_cache_core::domain::pack::{Pack, PackState};
use oxabl_cache_core::ids::{CacheId, ClientRef, PackId, PrincipalId};
use oxabl_cache_core::ports::metadata::{PacksTx, RecordedCommit};
use oxabl_cache_core::{Error, Result};
use rusqlite::{OptionalExtension, params};

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
}
