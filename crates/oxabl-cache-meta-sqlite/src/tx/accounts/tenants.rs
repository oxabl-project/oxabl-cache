//! Tenant rows.

use crate::db::{DbResult, RowExt, int};
use crate::tx::SqliteTx;
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::tenant::{Tenant, TenantStatus};
use oxabl_cache_core::ids::TenantId;
use oxabl_cache_core::ports::metadata::TenantsTx;
use rusqlite::{OptionalExtension, params};

impl TenantsTx for SqliteTx<'_> {
    fn tenant(&self, id: &TenantId) -> Result<Option<Tenant>> {
        self.conn
            .prepare_cached(
                "SELECT tenant_id, name, status, hard_bytes, egress_bytes, rate, burst, created_at
                 FROM tenants WHERE tenant_id = ?1",
            )
            .db()?
            .query_row([id.as_bytes().as_slice()], |row| {
                Ok(Tenant {
                    id: TenantId::from_bytes(row.blob(0)?),
                    name: row.get(1)?,
                    status: if row.get::<_, i64>(2)? == 0 {
                        TenantStatus::Active
                    } else {
                        TenantStatus::Suspended
                    },
                    hard_bytes: row.opt_unsigned(3)?,
                    egress_bytes: row.opt_unsigned(4)?,
                    rate: row.small(5)?,
                    burst: row.small(6)?,
                    created_at: row.get(7)?,
                })
            })
            .optional()
            .db()
    }

    fn insert_tenant(&mut self, tenant: &Tenant) -> Result<bool> {
        let status = i64::from(tenant.status == TenantStatus::Suspended);
        let changed = self
            .conn
            .prepare_cached(
                "INSERT INTO tenants
                   (tenant_id, name, status, hard_bytes, egress_bytes, rate, burst, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (tenant_id) DO NOTHING",
            )
            .db()?
            .execute(params![
                tenant.id.as_bytes().as_slice(),
                tenant.name,
                status,
                tenant.hard_bytes.map(int),
                tenant.egress_bytes.map(int),
                tenant.rate,
                tenant.burst,
                tenant.created_at,
            ])
            .db()?;
        Ok(changed == 1)
    }

    fn set_tenant_status(&mut self, id: &TenantId, status: TenantStatus) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                "UPDATE tenants SET status = ?2 WHERE tenant_id = ?1",
                params![
                    id.as_bytes().as_slice(),
                    i64::from(status == TenantStatus::Suspended)
                ],
            )
            .db()?;
        Ok(changed == 1)
    }

    fn tenant_bytes(&self, tenant: &TenantId) -> Result<(u64, u64)> {
        self.conn
            .prepare_cached(
                "SELECT COALESCE(SUM(live_bytes), 0), COALESCE(SUM(pending_bytes), 0)
                 FROM caches WHERE tenant_id = ?1",
            )
            .db()?
            .query_row([tenant.as_bytes().as_slice()], |row| {
                Ok((row.unsigned(0)?, row.unsigned(1)?))
            })
            .db()
    }
}
