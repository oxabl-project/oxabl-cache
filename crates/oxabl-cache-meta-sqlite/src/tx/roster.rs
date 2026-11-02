//! Owner keys, rosters and the dictionary pointer.

use super::SqliteTx;
use crate::db::{DbResult, RowExt, int};
use oxabl_cache_core::domain::roster::{
    MemberRole, PublicKey, StoredDictionaryPointer, StoredRoster,
};
use oxabl_cache_core::ids::{CacheId, SegmentId};
use oxabl_cache_core::ports::metadata::RosterTx;
use oxabl_cache_core::{Error, Result};
use rusqlite::{OptionalExtension, params};

impl RosterTx for SqliteTx<'_> {
    fn set_owner_keys(&mut self, cache: &CacheId, keys: &[PublicKey]) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM cache_owner_keys WHERE cache_id = ?1",
                [cache.as_bytes().as_slice()],
            )
            .db()?;
        let mut insert = self
            .conn
            .prepare_cached(
                "INSERT INTO cache_owner_keys (cache_id, sign_key) VALUES (?1, ?2)
                 ON CONFLICT (cache_id, sign_key) DO NOTHING",
            )
            .db()?;
        for key in keys {
            insert
                .execute(params![cache.as_bytes().as_slice(), key.as_slice()])
                .db()?;
        }
        Ok(())
    }

    fn owner_keys(&self, cache: &CacheId) -> Result<Vec<PublicKey>> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT sign_key FROM cache_owner_keys WHERE cache_id = ?1")
            .db()?;
        let rows = statement
            .query_map([cache.as_bytes().as_slice()], |row| row.blob::<32>(0))
            .db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }

    fn roster(&self, cache: &CacheId) -> Result<Option<StoredRoster>> {
        self.conn
            .prepare_cached("SELECT version, epoch, roster FROM rosters WHERE cache_id = ?1")
            .db()?
            .query_row([cache.as_bytes().as_slice()], |row| {
                Ok(StoredRoster {
                    version: row.unsigned(0)?,
                    epoch: row.small(1)?,
                    bytes: row.get(2)?,
                })
            })
            .optional()
            .db()
    }

    fn replace_roster(
        &mut self,
        cache: &CacheId,
        expected_version: u64,
        roster: &StoredRoster,
        signers: &[(PublicKey, MemberRole)],
        now: i64,
    ) -> Result<bool> {
        let moved = self
            .conn
            .prepare_cached(
                "UPDATE caches SET roster_version = ?2, epoch = ?3
                 WHERE cache_id = ?1 AND roster_version = ?4",
            )
            .db()?
            .execute(params![
                cache.as_bytes().as_slice(),
                int(roster.version),
                roster.epoch,
                int(expected_version)
            ])
            .db()?;
        if moved != 1 {
            return Ok(false);
        }
        self.conn
            .prepare_cached(
                "INSERT INTO rosters (cache_id, version, epoch, roster, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (cache_id) DO UPDATE SET
                   version = excluded.version, epoch = excluded.epoch,
                   roster = excluded.roster, updated_at = excluded.updated_at",
            )
            .db()?
            .execute(params![
                cache.as_bytes().as_slice(),
                int(roster.version),
                roster.epoch,
                roster.bytes,
                now
            ])
            .db()?;
        self.conn
            .execute(
                "DELETE FROM roster_signers WHERE cache_id = ?1",
                [cache.as_bytes().as_slice()],
            )
            .db()?;
        let mut insert = self
            .conn
            .prepare_cached(
                "INSERT INTO roster_signers (cache_id, sign_key, role) VALUES (?1, ?2, ?3)
                 ON CONFLICT (cache_id, sign_key, role) DO NOTHING",
            )
            .db()?;
        for (key, role) in signers {
            insert
                .execute(params![
                    cache.as_bytes().as_slice(),
                    key.as_slice(),
                    role.code()
                ])
                .db()?;
        }
        Ok(true)
    }

    fn signer_roles(&self, cache: &CacheId, key: &PublicKey) -> Result<Vec<MemberRole>> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT role FROM roster_signers WHERE cache_id = ?1 AND sign_key = ?2")
            .db()?;
        let rows = statement
            .query_map([cache.as_bytes().as_slice(), key.as_slice()], |row| {
                row.get::<_, i64>(0)
            })
            .db()?;
        rows.map(|code| {
            u8::try_from(code.db()?)
                .ok()
                .and_then(MemberRole::from_code)
                .ok_or_else(|| Error::internal("stored roster role is malformed"))
        })
        .collect()
    }

    fn dictionary_pointer(&self, cache: &CacheId) -> Result<Option<StoredDictionaryPointer>> {
        self.conn
            .prepare_cached(
                "SELECT version, epoch, segment_id, pointer
                 FROM dictionary_pointers WHERE cache_id = ?1",
            )
            .db()?
            .query_row([cache.as_bytes().as_slice()], |row| {
                Ok(StoredDictionaryPointer {
                    version: row.unsigned(0)?,
                    epoch: row.small(1)?,
                    segment_id: SegmentId::from_bytes(row.blob(2)?),
                    bytes: row.get(3)?,
                })
            })
            .optional()
            .db()
    }

    fn delete_dictionary_pointer(&mut self, cache: &CacheId) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM dictionary_pointers WHERE cache_id = ?1",
                [cache.as_bytes().as_slice()],
            )
            .db()?;
        Ok(())
    }

    fn replace_dictionary_pointer(
        &mut self,
        cache: &CacheId,
        expected_version: u64,
        pointer: &StoredDictionaryPointer,
        now: i64,
    ) -> Result<bool> {
        let changed = if expected_version == 0 {
            self.conn
                .prepare_cached(
                    "INSERT INTO dictionary_pointers
                       (cache_id, version, epoch, segment_id, pointer, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (cache_id) DO NOTHING",
                )
                .db()?
                .execute(params![
                    cache.as_bytes().as_slice(),
                    int(pointer.version),
                    pointer.epoch,
                    pointer.segment_id.as_bytes().as_slice(),
                    pointer.bytes,
                    now
                ])
                .db()?
        } else {
            self.conn
                .prepare_cached(
                    "UPDATE dictionary_pointers
                     SET version = ?2, epoch = ?3, segment_id = ?4, pointer = ?5, updated_at = ?6
                     WHERE cache_id = ?1 AND version = ?7",
                )
                .db()?
                .execute(params![
                    cache.as_bytes().as_slice(),
                    int(pointer.version),
                    pointer.epoch,
                    pointer.segment_id.as_bytes().as_slice(),
                    pointer.bytes,
                    now,
                    int(expected_version)
                ])
                .db()?
        };
        Ok(changed == 1)
    }
}
