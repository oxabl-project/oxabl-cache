//! Tombstones and batched removal of deleted tenants and caches.

use super::{PgTx, RowExt, args, int};
use oxabl_cache_core::ids::{CacheId, TenantId};
use oxabl_cache_core::ports::metadata::TeardownTx;
use oxabl_cache_core::{Error, Result};

/// Tables holding rows of one cache, in foreign-key order.
const CACHE_ROWS: &[&str] = &[
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
        "policy_id IN (SELECT policy_id FROM workload_policies WHERE tenant_id = $1)",
    ),
    (
        "workload_policy_grants",
        "policy_id IN (SELECT policy_id FROM workload_policies WHERE tenant_id = $1)",
    ),
    ("workload_policies", "tenant_id = $1"),
    ("oidc_issuer_claims", "tenant_id = $1"),
    ("oidc_issuers", "tenant_id = $1"),
    (
        "token_grants",
        "credential_id IN (SELECT credential_id FROM tokens WHERE tenant_id = $1)",
    ),
    ("tokens", "tenant_id = $1"),
    (
        "session_grants",
        "credential_id IN (SELECT credential_id FROM sessions WHERE tenant_id = $1)",
    ),
    ("sessions", "tenant_id = $1"),
    ("grants", "tenant_id = $1"),
    ("users", "tenant_id = $1"),
];

impl PgTx<'_> {
    fn delete_batch(&self, table: &str, condition: &str, id: &[u8], limit: i64) -> Result<u64> {
        self.execute(
            &format!(
                "DELETE FROM {table} WHERE ctid IN
                   (SELECT ctid FROM {table} WHERE {condition} LIMIT $2)"
            ),
            args![id, limit],
        )
    }
}

impl TeardownTx for PgTx<'_> {
    fn tombstone_cache(&mut self, tenant: &TenantId, cache: &CacheId, now: i64) -> Result<bool> {
        let changed = self.execute(
            "UPDATE caches SET deleted_at = $3
             WHERE cache_id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
            args![
                cache.as_bytes().as_slice(),
                tenant.as_bytes().as_slice(),
                now
            ],
        )?;
        Ok(changed == 1)
    }

    fn tombstone_tenant(&mut self, tenant: &TenantId, now: i64) -> Result<bool> {
        let changed = self.execute(
            "UPDATE tenants SET deleted_at = $2 WHERE tenant_id = $1 AND deleted_at IS NULL",
            args![tenant.as_bytes().as_slice(), now],
        )?;
        if changed == 1 {
            self.execute(
                "UPDATE caches SET deleted_at = $2 WHERE tenant_id = $1 AND deleted_at IS NULL",
                args![tenant.as_bytes().as_slice(), now],
            )?;
        }
        Ok(changed == 1)
    }

    fn tenant_name_taken(&self, name: &str) -> Result<bool> {
        self.query_one(
            "SELECT EXISTS (SELECT 1 FROM tenants WHERE name = $1)",
            args![name],
        )?
        .flag(0)
    }

    fn cache_name_taken(&self, tenant: &TenantId, name: &str) -> Result<bool> {
        self.query_one(
            "SELECT EXISTS (SELECT 1 FROM caches WHERE tenant_id = $1 AND display_name = $2)",
            args![tenant.as_bytes().as_slice(), name],
        )?
        .flag(0)
    }

    fn tombstoned_caches(&self, before: i64, limit: usize) -> Result<Vec<(CacheId, TenantId)>> {
        self.query(
            "SELECT cache_id, tenant_id FROM caches
             WHERE deleted_at IS NOT NULL AND deleted_at <= $1
             ORDER BY deleted_at, cache_id LIMIT $2",
            args![before, int(limit as u64)],
        )?
        .iter()
        .map(|row| {
            Ok((
                CacheId::from_bytes(row.blob(0)?),
                TenantId::from_bytes(row.blob(1)?),
            ))
        })
        .collect()
    }

    fn tombstoned_tenants_without_caches(
        &self,
        before: i64,
        limit: usize,
    ) -> Result<Vec<TenantId>> {
        self.query(
            "SELECT tenant_id FROM tenants t
             WHERE deleted_at IS NOT NULL AND deleted_at <= $1
               AND NOT EXISTS (SELECT 1 FROM caches c WHERE c.tenant_id = t.tenant_id)
             ORDER BY deleted_at, tenant_id LIMIT $2",
            args![before, int(limit as u64)],
        )?
        .iter()
        .map(|row| Ok(TenantId::from_bytes(row.blob(0)?)))
        .collect()
    }

    fn drop_cache_rows(&mut self, cache: &CacheId, limit: usize) -> Result<bool> {
        let id = cache.as_bytes().as_slice();
        let tombstoned = self
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM caches WHERE cache_id = $1 AND deleted_at IS NOT NULL)",
                args![id],
            )?
            .flag(0)?;
        if !tombstoned {
            return Err(Error::internal("rows of a cache that is not tombstoned"));
        }
        let budget = int(limit as u64);
        for table in CACHE_ROWS {
            if int(self.delete_batch(table, "cache_id = $1", id, budget)?) >= budget {
                return Ok(false);
            }
        }
        self.execute("DELETE FROM caches WHERE cache_id = $1", args![id])?;
        Ok(true)
    }

    fn drop_tenant_rows(&mut self, tenant: &TenantId, limit: usize) -> Result<bool> {
        let id = tenant.as_bytes().as_slice();
        let ready = self
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM tenants WHERE tenant_id = $1 AND deleted_at IS NOT NULL)
                    AND NOT EXISTS (SELECT 1 FROM caches WHERE tenant_id = $1)",
                args![id],
            )?
            .flag(0)?;
        if !ready {
            return Err(Error::internal(
                "rows of a tenant that is not tombstoned or still owns caches",
            ));
        }
        let budget = int(limit as u64);
        for (table, condition) in TENANT_ROWS {
            if int(self.delete_batch(table, condition, id, budget)?) >= budget {
                return Ok(false);
            }
        }
        self.execute("DELETE FROM tenants WHERE tenant_id = $1", args![id])?;
        Ok(true)
    }
}
