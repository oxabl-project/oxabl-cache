//! Standing grants.

use crate::tx::{PgTx, RowExt, args};
use oxabl_cache_core::domain::scope::{Grant, ScopeSet};
use oxabl_cache_core::ids::{CacheId, PrincipalId, TenantId};
use oxabl_cache_core::ports::metadata::GrantsTx;
use oxabl_cache_core::{Error, Result};

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

impl GrantsTx for PgTx<'_> {
    fn set_grant(
        &mut self,
        tenant: &TenantId,
        principal: &PrincipalId,
        grant: &Grant,
    ) -> Result<()> {
        let cache = grant.cache.map_or(NO_CACHE, |cache| *cache.as_bytes());
        self.execute(
            "INSERT INTO grants (principal_id, tenant_id, cache_id, scopes)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (principal_id, cache_id) DO UPDATE SET scopes = excluded.scopes",
            args![
                principal.as_bytes().as_slice(),
                tenant.as_bytes().as_slice(),
                cache.as_slice(),
                i64::from(grant.scopes.bits()),
            ],
        )?;
        Ok(())
    }

    fn grants_of(&self, principal: &PrincipalId) -> Result<Vec<Grant>> {
        self.query(
            "SELECT cache_id, scopes FROM grants WHERE principal_id = $1 ORDER BY cache_id",
            args![principal.as_bytes().as_slice()],
        )?
        .iter()
        .map(|row| grant_of(row.blob(0)?, row.int(1)?))
        .collect()
    }
}
