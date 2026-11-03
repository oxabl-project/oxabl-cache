//! Standing grants.

use crate::db::{DbResult, RowExt};
use crate::tx::SqliteTx;
use oxabl_cache_core::domain::scope::{Grant, ScopeSet};
use oxabl_cache_core::ids::{CacheId, PrincipalId, TenantId};
use oxabl_cache_core::ports::metadata::GrantsTx;
use oxabl_cache_core::{Error, Result};
use rusqlite::params;

pub(in crate::tx) const NO_CACHE: [u8; CacheId::LEN] = [0; CacheId::LEN];

pub(in crate::tx) fn grant_of(cache: [u8; CacheId::LEN], scopes: i64) -> Result<Grant> {
    let scopes = u8::try_from(scopes)
        .ok()
        .and_then(ScopeSet::from_bits)
        .ok_or_else(|| Error::internal("stored scopes are malformed"))?;
    Ok(Grant {
        cache: (cache != NO_CACHE).then(|| CacheId::from_bytes(cache)),
        scopes,
    })
}

impl GrantsTx for SqliteTx<'_> {
    fn set_grant(
        &mut self,
        tenant: &TenantId,
        principal: &PrincipalId,
        grant: &Grant,
    ) -> Result<()> {
        let cache = grant.cache.map_or(NO_CACHE, |cache| *cache.as_bytes());
        self.conn
            .prepare_cached(
                "INSERT INTO grants (principal_id, tenant_id, cache_id, scopes)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (principal_id, cache_id) DO UPDATE SET scopes = excluded.scopes",
            )
            .db()?
            .execute(params![
                principal.as_bytes().as_slice(),
                tenant.as_bytes().as_slice(),
                cache.as_slice(),
                grant.scopes.bits(),
            ])
            .db()?;
        Ok(())
    }

    fn grants_of(&self, principal: &PrincipalId) -> Result<Vec<Grant>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT cache_id, scopes FROM grants WHERE principal_id = ?1 ORDER BY cache_id",
            )
            .db()?;
        let rows = statement
            .query_map([principal.as_bytes().as_slice()], |row| {
                Ok((row.blob::<{ CacheId::LEN }>(0)?, row.get::<_, i64>(1)?))
            })
            .db()?;
        rows.map(|row| {
            let (cache, scopes) = row.db()?;
            grant_of(cache, scopes)
        })
        .collect()
    }
}
