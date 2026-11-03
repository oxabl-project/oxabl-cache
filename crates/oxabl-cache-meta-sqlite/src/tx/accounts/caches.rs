//! Cache rows and byte counters.

use crate::db::{DbResult, RowExt, int};
use crate::tx::SqliteTx;
use oxabl_cache_core::domain::cache::Cache;
use oxabl_cache_core::ids::{CacheId, TenantId};
use oxabl_cache_core::ports::metadata::CachesTx;
use oxabl_cache_core::{Error, Result};
use rusqlite::{OptionalExtension, Row, params};

pub(in crate::tx) const CACHE_COLUMNS: &str = "cache_id, tenant_id, display_name, retention_days, hard_bytes, \
     epoch, roster_version, live_bytes, pending_bytes, created_at";

pub(in crate::tx) fn cache_of(row: &Row<'_>) -> rusqlite::Result<Cache> {
    Ok(Cache {
        id: CacheId::from_bytes(row.blob(0)?),
        tenant: TenantId::from_bytes(row.blob(1)?),
        display_name: row.get(2)?,
        retention_days: row.small(3)?,
        hard_bytes: row.unsigned(4)?,
        epoch: row.small(5)?,
        roster_version: row.unsigned(6)?,
        live_bytes: row.unsigned(7)?,
        pending_bytes: row.unsigned(8)?,
        created_at: row.get(9)?,
    })
}

impl CachesTx for SqliteTx<'_> {
    fn insert_cache(&mut self, cache: &Cache) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(&format!(
                "INSERT INTO caches ({CACHE_COLUMNS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT (cache_id) DO NOTHING"
            ))
            .db()?
            .execute(params![
                cache.id.as_bytes().as_slice(),
                cache.tenant.as_bytes().as_slice(),
                cache.display_name,
                cache.retention_days,
                int(cache.hard_bytes),
                cache.epoch,
                int(cache.roster_version),
                int(cache.live_bytes),
                int(cache.pending_bytes),
                cache.created_at,
            ])
            .db()?;
        Ok(changed == 1)
    }

    fn cache(&self, tenant: &TenantId, id: &CacheId) -> Result<Option<Cache>> {
        self.conn
            .prepare_cached(&format!(
                "SELECT {CACHE_COLUMNS} FROM caches WHERE cache_id = ?1 AND tenant_id = ?2"
            ))
            .db()?
            .query_row(
                [id.as_bytes().as_slice(), tenant.as_bytes().as_slice()],
                cache_of,
            )
            .optional()
            .db()
    }

    fn cache_tenant(&self, id: &CacheId) -> Result<Option<TenantId>> {
        self.conn
            .prepare_cached("SELECT tenant_id FROM caches WHERE cache_id = ?1")
            .db()?
            .query_row([id.as_bytes().as_slice()], |row| {
                Ok(TenantId::from_bytes(row.blob(0)?))
            })
            .optional()
            .db()
    }

    fn all_caches(&self) -> Result<Vec<Cache>> {
        let mut statement = self
            .conn
            .prepare_cached(&format!(
                "SELECT {CACHE_COLUMNS} FROM caches ORDER BY cache_id"
            ))
            .db()?;
        let rows = statement.query_map([], cache_of).db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }

    fn adjust_cache_bytes(&mut self, cache: &CacheId, live: i64, pending: i64) -> Result<()> {
        let changed = self
            .conn
            .prepare_cached(
                "UPDATE caches SET live_bytes = live_bytes + ?2, pending_bytes = pending_bytes + ?3
                 WHERE cache_id = ?1",
            )
            .db()?
            .execute(params![cache.as_bytes().as_slice(), live, pending])
            .db()?;
        if changed == 1 {
            Ok(())
        } else {
            Err(Error::internal(
                "byte counters adjusted for an unknown cache",
            ))
        }
    }
}
