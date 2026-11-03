//! Owner keys, rosters and the dictionary pointer.

use super::{PgTx, RowExt, args, byte_array, int};
use oxabl_cache_core::domain::roster::{
    MemberRole, PublicKey, StoredDictionaryPointer, StoredRoster,
};
use oxabl_cache_core::ids::{CacheId, SegmentId};
use oxabl_cache_core::ports::metadata::RosterTx;
use oxabl_cache_core::{Error, Result};

impl RosterTx for PgTx<'_> {
    fn set_owner_keys(&mut self, cache: &CacheId, keys: &[PublicKey]) -> Result<()> {
        self.execute(
            "DELETE FROM cache_owner_keys WHERE cache_id = $1",
            args![cache.as_bytes().as_slice()],
        )?;
        let keys = byte_array(keys.iter().map(|key| key.as_slice()));
        self.execute(
            "INSERT INTO cache_owner_keys (cache_id, sign_key)
             SELECT $1, k FROM unnest($2::bytea[]) AS k
             ON CONFLICT (cache_id, sign_key) DO NOTHING",
            args![cache.as_bytes().as_slice(), keys],
        )?;
        Ok(())
    }

    fn owner_keys(&self, cache: &CacheId) -> Result<Vec<PublicKey>> {
        self.query(
            "SELECT sign_key FROM cache_owner_keys WHERE cache_id = $1",
            args![cache.as_bytes().as_slice()],
        )?
        .iter()
        .map(|row| row.blob::<32>(0))
        .collect()
    }

    fn roster(&self, cache: &CacheId) -> Result<Option<StoredRoster>> {
        self.query_opt(
            "SELECT version, epoch, roster FROM rosters WHERE cache_id = $1",
            args![cache.as_bytes().as_slice()],
        )?
        .map(|row| {
            Ok(StoredRoster {
                version: row.unsigned(0)?,
                epoch: row.small(1)?,
                bytes: row.bytes(2)?,
            })
        })
        .transpose()
    }

    fn replace_roster(
        &mut self,
        cache: &CacheId,
        expected_version: u64,
        roster: &StoredRoster,
        signers: &[(PublicKey, MemberRole)],
        now: i64,
    ) -> Result<bool> {
        let moved = self.execute(
            "UPDATE caches SET roster_version = $2, epoch = $3
             WHERE cache_id = $1 AND roster_version = $4",
            args![
                cache.as_bytes().as_slice(),
                int(roster.version),
                i64::from(roster.epoch),
                int(expected_version)
            ],
        )?;
        if moved != 1 {
            return Ok(false);
        }
        self.execute(
            "INSERT INTO rosters (cache_id, version, epoch, roster, updated_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (cache_id) DO UPDATE SET
               version = excluded.version, epoch = excluded.epoch,
               roster = excluded.roster, updated_at = excluded.updated_at",
            args![
                cache.as_bytes().as_slice(),
                int(roster.version),
                i64::from(roster.epoch),
                roster.bytes,
                now
            ],
        )?;
        self.execute(
            "DELETE FROM roster_signers WHERE cache_id = $1",
            args![cache.as_bytes().as_slice()],
        )?;
        for (key, role) in signers {
            self.execute(
                "INSERT INTO roster_signers (cache_id, sign_key, role) VALUES ($1, $2, $3)
                 ON CONFLICT (cache_id, sign_key, role) DO NOTHING",
                args![
                    cache.as_bytes().as_slice(),
                    key.as_slice(),
                    i64::from(role.code())
                ],
            )?;
        }
        Ok(true)
    }

    fn signer_roles(&self, cache: &CacheId, key: &PublicKey) -> Result<Vec<MemberRole>> {
        self.query(
            "SELECT role FROM roster_signers WHERE cache_id = $1 AND sign_key = $2",
            args![cache.as_bytes().as_slice(), key.as_slice()],
        )?
        .iter()
        .map(|row| {
            u8::try_from(row.int(0)?)
                .ok()
                .and_then(MemberRole::from_code)
                .ok_or_else(|| Error::internal("stored roster role is malformed"))
        })
        .collect()
    }

    fn dictionary_pointer(&self, cache: &CacheId) -> Result<Option<StoredDictionaryPointer>> {
        self.query_opt(
            "SELECT version, epoch, segment_id, pointer
             FROM dictionary_pointers WHERE cache_id = $1",
            args![cache.as_bytes().as_slice()],
        )?
        .map(|row| {
            Ok(StoredDictionaryPointer {
                version: row.unsigned(0)?,
                epoch: row.small(1)?,
                segment_id: SegmentId::from_bytes(row.blob(2)?),
                bytes: row.bytes(3)?,
            })
        })
        .transpose()
    }

    fn delete_dictionary_pointer(&mut self, cache: &CacheId) -> Result<()> {
        self.execute(
            "DELETE FROM dictionary_pointers WHERE cache_id = $1",
            args![cache.as_bytes().as_slice()],
        )?;
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
            self.execute(
                "INSERT INTO dictionary_pointers
                   (cache_id, version, epoch, segment_id, pointer, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (cache_id) DO NOTHING",
                args![
                    cache.as_bytes().as_slice(),
                    int(pointer.version),
                    i64::from(pointer.epoch),
                    pointer.segment_id.as_bytes().as_slice(),
                    pointer.bytes,
                    now
                ],
            )?
        } else {
            self.execute(
                "UPDATE dictionary_pointers
                 SET version = $2, epoch = $3, segment_id = $4, pointer = $5, updated_at = $6
                 WHERE cache_id = $1 AND version = $7",
                args![
                    cache.as_bytes().as_slice(),
                    int(pointer.version),
                    i64::from(pointer.epoch),
                    pointer.segment_id.as_bytes().as_slice(),
                    pointer.bytes,
                    now,
                    int(expected_version)
                ],
            )?
        };
        Ok(changed == 1)
    }
}
