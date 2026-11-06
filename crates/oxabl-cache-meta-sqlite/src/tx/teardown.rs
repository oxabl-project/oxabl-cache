//! Tombstones and batched removal of deleted tenants and caches.

use super::SqliteTx;
use crate::db::{DbResult, RowExt, int};
use oxabl_cache_core::ids::{CacheId, TenantId};
use oxabl_cache_core::ports::metadata::TeardownTx;
use oxabl_cache_core::{Error, Result};
use rusqlite::params;

/// Tables holding rows of one cache, in foreign-key order.
const CACHE_ROWS: &[&str] = &[
    "telemetry",
    "entry_access",
    "entries",
    "dictionary_pointers",
    "segments",
    "packs",
    "scrub_queue",
    "roster_signers",
    "rosters",
    "cache_owner_keys",
    "grants",
    "token_grants",
    "session_grants",
    "workload_policy_grants",
];

/// Tables of one tenant with the condition that selects its rows, in foreign-key order.
const TENANT_ROWS: &[(&str, &str)] = &[
    (
        "workload_policy_claims",
        "policy_id IN (SELECT policy_id FROM workload_policies WHERE tenant_id = ?1)",
    ),
    (
        "workload_policy_grants",
        "policy_id IN (SELECT policy_id FROM workload_policies WHERE tenant_id = ?1)",
    ),
    ("workload_policies", "tenant_id = ?1"),
    ("oidc_issuer_claims", "tenant_id = ?1"),
    ("oidc_issuers", "tenant_id = ?1"),
    (
        "token_grants",
        "credential_id IN (SELECT credential_id FROM tokens WHERE tenant_id = ?1)",
    ),
    ("tokens", "tenant_id = ?1"),
    (
        "session_grants",
        "credential_id IN (SELECT credential_id FROM sessions WHERE tenant_id = ?1)",
    ),
    ("sessions", "tenant_id = ?1"),
    ("grants", "tenant_id = ?1"),
    ("users", "tenant_id = ?1"),
];

fn delete_batch(
    conn: &rusqlite::Connection,
    table: &str,
    condition: &str,
    id: &[u8],
    limit: i64,
) -> Result<usize> {
    conn.execute(
        &format!(
            "DELETE FROM {table} WHERE rowid IN
               (SELECT rowid FROM {table} WHERE {condition} LIMIT ?2)"
        ),
        params![id, limit],
    )
    .db()
}

impl TeardownTx for SqliteTx<'_> {
    fn tombstone_cache(&mut self, tenant: &TenantId, cache: &CacheId, now: i64) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                "UPDATE caches SET deleted_at = ?3
                 WHERE cache_id = ?1 AND tenant_id = ?2 AND deleted_at IS NULL",
                params![
                    cache.as_bytes().as_slice(),
                    tenant.as_bytes().as_slice(),
                    now
                ],
            )
            .db()?;
        Ok(changed == 1)
    }

    fn tombstone_tenant(&mut self, tenant: &TenantId, now: i64) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                "UPDATE tenants SET deleted_at = ?2 WHERE tenant_id = ?1 AND deleted_at IS NULL",
                params![tenant.as_bytes().as_slice(), now],
            )
            .db()?;
        if changed == 1 {
            self.conn
                .execute(
                    "UPDATE caches SET deleted_at = ?2 WHERE tenant_id = ?1 AND deleted_at IS NULL",
                    params![tenant.as_bytes().as_slice(), now],
                )
                .db()?;
        }
        Ok(changed == 1)
    }

    fn tenant_name_taken(&self, name: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM tenants WHERE name = ?1)",
                [name],
                |row| row.get(0),
            )
            .db()
    }

    fn cache_name_taken(&self, tenant: &TenantId, name: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM caches WHERE tenant_id = ?1 AND display_name = ?2)",
                params![tenant.as_bytes().as_slice(), name],
                |row| row.get(0),
            )
            .db()
    }

    fn tombstoned_caches(&self, before: i64, limit: usize) -> Result<Vec<(CacheId, TenantId)>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT cache_id, tenant_id FROM caches
                 WHERE deleted_at IS NOT NULL AND deleted_at <= ?1
                 ORDER BY deleted_at, cache_id LIMIT ?2",
            )
            .db()?;
        let rows = statement
            .query_map(params![before, int(limit as u64)], |row| {
                Ok((
                    CacheId::from_bytes(row.blob(0)?),
                    TenantId::from_bytes(row.blob(1)?),
                ))
            })
            .db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }

    fn tombstoned_tenants_without_caches(
        &self,
        before: i64,
        limit: usize,
    ) -> Result<Vec<TenantId>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT tenant_id FROM tenants t
                 WHERE deleted_at IS NOT NULL AND deleted_at <= ?1
                   AND NOT EXISTS (SELECT 1 FROM caches c WHERE c.tenant_id = t.tenant_id)
                 ORDER BY deleted_at, tenant_id LIMIT ?2",
            )
            .db()?;
        let rows = statement
            .query_map(params![before, int(limit as u64)], |row| {
                Ok(TenantId::from_bytes(row.blob(0)?))
            })
            .db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }

    fn drop_cache_rows(&mut self, cache: &CacheId, limit: usize) -> Result<bool> {
        let id = cache.as_bytes().as_slice();
        let tombstoned: bool = self
            .conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM caches WHERE cache_id = ?1 AND deleted_at IS NOT NULL)",
                [id],
                |row| row.get(0),
            )
            .db()?;
        if !tombstoned {
            return Err(Error::internal("rows of a cache that is not tombstoned"));
        }
        let budget = int(limit as u64);
        for table in CACHE_ROWS {
            if int(delete_batch(self.conn, table, "cache_id = ?1", id, budget)? as u64) >= budget {
                return Ok(false);
            }
        }
        self.conn
            .execute("DELETE FROM caches WHERE cache_id = ?1", [id])
            .db()?;
        Ok(true)
    }

    fn drop_tenant_rows(&mut self, tenant: &TenantId, limit: usize) -> Result<bool> {
        let id = tenant.as_bytes().as_slice();
        let ready: bool = self
            .conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM tenants WHERE tenant_id = ?1 AND deleted_at IS NOT NULL)
                    AND NOT EXISTS (SELECT 1 FROM caches WHERE tenant_id = ?1)",
                [id],
                |row| row.get(0),
            )
            .db()?;
        if !ready {
            return Err(Error::internal(
                "rows of a tenant that is not tombstoned or still owns caches",
            ));
        }
        let budget = int(limit as u64);
        for (table, condition) in TENANT_ROWS {
            if int(delete_batch(self.conn, table, condition, id, budget)? as u64) >= budget {
                return Ok(false);
            }
        }
        self.conn
            .execute("DELETE FROM tenants WHERE tenant_id = ?1", [id])
            .db()?;
        Ok(true)
    }
}
