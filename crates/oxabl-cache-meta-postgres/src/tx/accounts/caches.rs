//! Cache rows and byte counters.

use crate::tx::{PgTx, RowExt, args, int};
use oxabl_cache_core::domain::cache::Cache;
use oxabl_cache_core::ids::{CacheId, TenantId};
use oxabl_cache_core::ports::metadata::CachesTx;
use oxabl_cache_core::{Error, Result};
use tokio_postgres::Row;

pub(in crate::tx) const CACHE_COLUMNS: &str = "cache_id, tenant_id, display_name, retention_days, \
     hard_bytes, epoch, roster_version, live_bytes, pending_bytes, created_at";

pub(in crate::tx) fn cache_of(row: &Row) -> Result<Cache> {
    Ok(Cache {
        id: CacheId::from_bytes(row.blob(0)?),
        tenant: TenantId::from_bytes(row.blob(1)?),
        display_name: row.text(2)?,
        retention_days: row.small(3)?,
        hard_bytes: row.unsigned(4)?,
        epoch: row.small(5)?,
        roster_version: row.unsigned(6)?,
        live_bytes: row.unsigned(7)?,
        pending_bytes: row.unsigned(8)?,
        created_at: row.int(9)?,
    })
}

impl CachesTx for PgTx<'_> {
    fn insert_cache(&mut self, cache: &Cache) -> Result<bool> {
        let changed = self.execute(
            &format!(
                "INSERT INTO caches ({CACHE_COLUMNS})
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT (cache_id) DO NOTHING"
            ),
            args![
                cache.id.as_bytes().as_slice(),
                cache.tenant.as_bytes().as_slice(),
                cache.display_name,
                i64::from(cache.retention_days),
                int(cache.hard_bytes),
                i64::from(cache.epoch),
                int(cache.roster_version),
                int(cache.live_bytes),
                int(cache.pending_bytes),
                cache.created_at,
            ],
        )?;
        Ok(changed == 1)
    }

    fn cache(&self, tenant: &TenantId, id: &CacheId) -> Result<Option<Cache>> {
        self.query_opt(
            &format!(
                "SELECT {CACHE_COLUMNS} FROM caches
                 WHERE cache_id = $1 AND tenant_id = $2 AND deleted_at IS NULL"
            ),
            args![id.as_bytes().as_slice(), tenant.as_bytes().as_slice()],
        )?
        .as_ref()
        .map(cache_of)
        .transpose()
    }

    fn cache_tenant(&self, id: &CacheId) -> Result<Option<TenantId>> {
        self.query_opt(
            "SELECT tenant_id FROM caches WHERE cache_id = $1 AND deleted_at IS NULL",
            args![id.as_bytes().as_slice()],
        )?
        .map(|row| Ok(TenantId::from_bytes(row.blob(0)?)))
        .transpose()
    }

    fn all_caches(&self) -> Result<Vec<Cache>> {
        self.query(
            &format!(
                "SELECT {CACHE_COLUMNS} FROM caches WHERE deleted_at IS NULL ORDER BY cache_id"
            ),
            args![],
        )?
        .iter()
        .map(cache_of)
        .collect()
    }

    fn adjust_cache_bytes(&mut self, cache: &CacheId, live: i64, pending: i64) -> Result<()> {
        let changed = self.execute(
            "UPDATE caches SET live_bytes = live_bytes + $2, pending_bytes = pending_bytes + $3
             WHERE cache_id = $1",
            args![cache.as_bytes().as_slice(), live, pending],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(Error::internal(
                "byte counters adjusted for an unknown cache",
            ))
        }
    }
}
