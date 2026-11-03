//! Tenant rows.

use crate::tx::{PgTx, RowExt, args, int};
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::tenant::{Tenant, TenantStatus};
use oxabl_cache_core::ids::TenantId;
use oxabl_cache_core::ports::metadata::TenantsTx;
use tokio_postgres::Row;

pub(in crate::tx) const TENANT_COLUMNS: &str =
    "tenant_id, name, status, hard_bytes, egress_bytes, rate, burst, created_at";

pub(in crate::tx) fn tenant_of(row: &Row) -> Result<Tenant> {
    Ok(Tenant {
        id: TenantId::from_bytes(row.blob(0)?),
        name: row.text(1)?,
        status: if row.int(2)? == 0 {
            TenantStatus::Active
        } else {
            TenantStatus::Suspended
        },
        hard_bytes: row.opt_unsigned(3)?,
        egress_bytes: row.opt_unsigned(4)?,
        rate: row.small(5)?,
        burst: row.small(6)?,
        created_at: row.int(7)?,
    })
}

impl TenantsTx for PgTx<'_> {
    fn tenant(&self, id: &TenantId) -> Result<Option<Tenant>> {
        self.query_opt(
            &format!(
                "SELECT {TENANT_COLUMNS} FROM tenants WHERE tenant_id = $1 AND deleted_at IS NULL"
            ),
            args![id.as_bytes().as_slice()],
        )?
        .as_ref()
        .map(tenant_of)
        .transpose()
    }

    fn insert_tenant(&mut self, tenant: &Tenant) -> Result<bool> {
        let status = i64::from(tenant.status == TenantStatus::Suspended);
        let changed = self.execute(
            &format!(
                "INSERT INTO tenants ({TENANT_COLUMNS})
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (tenant_id) DO NOTHING"
            ),
            args![
                tenant.id.as_bytes().as_slice(),
                tenant.name,
                status,
                tenant.hard_bytes.map(int),
                tenant.egress_bytes.map(int),
                i64::from(tenant.rate),
                i64::from(tenant.burst),
                tenant.created_at,
            ],
        )?;
        Ok(changed == 1)
    }

    fn set_tenant_status(&mut self, id: &TenantId, status: TenantStatus) -> Result<bool> {
        let changed = self.execute(
            "UPDATE tenants SET status = $2 WHERE tenant_id = $1 AND deleted_at IS NULL",
            args![
                id.as_bytes().as_slice(),
                i64::from(status == TenantStatus::Suspended)
            ],
        )?;
        Ok(changed == 1)
    }

    fn tenant_bytes(&self, tenant: &TenantId) -> Result<(u64, u64)> {
        let row = self.query_one(
            "SELECT COALESCE(SUM(live_bytes), 0)::bigint, COALESCE(SUM(pending_bytes), 0)::bigint
             FROM caches WHERE tenant_id = $1 AND deleted_at IS NULL",
            args![tenant.as_bytes().as_slice()],
        )?;
        Ok((row.unsigned(0)?, row.unsigned(1)?))
    }
}
