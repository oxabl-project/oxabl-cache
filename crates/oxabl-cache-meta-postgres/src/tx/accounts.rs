//! Tenants, caches, users, grants and credentials.

use super::{PgTx, RowExt, args, int};
use oxabl_cache_core::domain::cache::Cache;
use oxabl_cache_core::domain::credential::{Credential, CredentialKind};
use oxabl_cache_core::domain::scope::{Grant, ScopeSet};
use oxabl_cache_core::domain::tenant::{Tenant, TenantStatus};
use oxabl_cache_core::ids::{CacheId, CredentialId, PrincipalId, TenantId};
use oxabl_cache_core::ports::metadata::{AccountsTx, User};
use oxabl_cache_core::{Error, Result};
use tokio_postgres::Row;

const NO_CACHE: [u8; CacheId::LEN] = [0; CacheId::LEN];

pub(super) const CACHE_COLUMNS: &str = "cache_id, tenant_id, display_name, retention_days, \
     hard_bytes, epoch, roster_version, live_bytes, pending_bytes, created_at";

pub(super) const TENANT_COLUMNS: &str =
    "tenant_id, name, status, hard_bytes, egress_bytes, rate, burst, created_at";

pub(super) const USER_COLUMNS: &str =
    "principal_id, tenant_id, issuer, subject, label, created_at, disabled_at";

pub(super) fn cache_of(row: &Row) -> Result<Cache> {
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

pub(super) fn tenant_of(row: &Row) -> Result<Tenant> {
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

pub(super) fn user_of(row: &Row) -> Result<User> {
    Ok(User {
        id: PrincipalId::from_bytes(row.blob(0)?),
        tenant: TenantId::from_bytes(row.blob(1)?),
        issuer: row.text(2)?,
        subject: row.text(3)?,
        label: row.text(4)?,
        created_at: row.int(5)?,
        disabled_at: row.opt_int(6)?,
    })
}

pub(super) fn grant_of(cache: [u8; CacheId::LEN], scopes: i64) -> Result<Grant> {
    let scopes = u8::try_from(scopes)
        .ok()
        .and_then(ScopeSet::from_bits)
        .ok_or_else(|| Error::internal("stored scopes are malformed"))?;
    Ok(Grant {
        cache: (cache != NO_CACHE).then(|| CacheId::from_bytes(cache)),
        scopes,
    })
}

fn tables(kind: CredentialKind) -> (&'static str, &'static str) {
    match kind {
        CredentialKind::Token => ("tokens", "token_grants"),
        CredentialKind::Session => ("sessions", "session_grants"),
    }
}

impl AccountsTx for PgTx<'_> {
    fn tenant(&self, id: &TenantId) -> Result<Option<Tenant>> {
        self.query_opt(
            &format!("SELECT {TENANT_COLUMNS} FROM tenants WHERE tenant_id = $1"),
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
            "UPDATE tenants SET status = $2 WHERE tenant_id = $1",
            args![
                id.as_bytes().as_slice(),
                i64::from(status == TenantStatus::Suspended)
            ],
        )?;
        Ok(changed == 1)
    }

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
            &format!("SELECT {CACHE_COLUMNS} FROM caches WHERE cache_id = $1 AND tenant_id = $2"),
            args![id.as_bytes().as_slice(), tenant.as_bytes().as_slice()],
        )?
        .as_ref()
        .map(cache_of)
        .transpose()
    }

    fn cache_tenant(&self, id: &CacheId) -> Result<Option<TenantId>> {
        self.query_opt(
            "SELECT tenant_id FROM caches WHERE cache_id = $1",
            args![id.as_bytes().as_slice()],
        )?
        .map(|row| Ok(TenantId::from_bytes(row.blob(0)?)))
        .transpose()
    }

    fn all_caches(&self) -> Result<Vec<Cache>> {
        self.query(
            &format!("SELECT {CACHE_COLUMNS} FROM caches ORDER BY cache_id"),
            args![],
        )?
        .iter()
        .map(cache_of)
        .collect()
    }

    fn tenant_bytes(&self, tenant: &TenantId) -> Result<(u64, u64)> {
        let row = self.query_one(
            "SELECT COALESCE(SUM(live_bytes), 0)::bigint, COALESCE(SUM(pending_bytes), 0)::bigint
             FROM caches WHERE tenant_id = $1",
            args![tenant.as_bytes().as_slice()],
        )?;
        Ok((row.unsigned(0)?, row.unsigned(1)?))
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

    fn insert_user(&mut self, user: &User) -> Result<bool> {
        let changed = self.execute(
            &format!(
                "INSERT INTO users ({USER_COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT DO NOTHING"
            ),
            args![
                user.id.as_bytes().as_slice(),
                user.tenant.as_bytes().as_slice(),
                user.issuer,
                user.subject,
                user.label,
                user.created_at,
                user.disabled_at,
            ],
        )?;
        Ok(changed == 1)
    }

    fn user(&self, id: &PrincipalId) -> Result<Option<User>> {
        self.query_opt(
            &format!("SELECT {USER_COLUMNS} FROM users WHERE principal_id = $1"),
            args![id.as_bytes().as_slice()],
        )?
        .as_ref()
        .map(user_of)
        .transpose()
    }

    fn disable_user(&mut self, id: &PrincipalId, now: i64) -> Result<bool> {
        let changed = self.execute(
            "UPDATE users SET disabled_at = $2 WHERE principal_id = $1 AND disabled_at IS NULL",
            args![id.as_bytes().as_slice(), now],
        )?;
        Ok(changed == 1)
    }

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

    fn insert_credential(&mut self, credential: &Credential) -> Result<bool> {
        let (table, grants) = tables(credential.kind);
        let changed = self.execute(
            &format!(
                "INSERT INTO {table}
                   (credential_id, tenant_id, principal_id, name, secret_hash,
                    created_at, expires_at, revoked_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (credential_id) DO NOTHING"
            ),
            args![
                credential.id.as_bytes().as_slice(),
                credential.tenant.as_bytes().as_slice(),
                credential.principal.as_bytes().as_slice(),
                credential.name,
                credential.secret_hash.as_slice(),
                credential.created_at,
                credential.expires_at,
                credential.revoked_at,
            ],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        for grant in &credential.grants {
            let cache = grant.cache.map_or(NO_CACHE, |cache| *cache.as_bytes());
            self.execute(
                &format!(
                    "INSERT INTO {grants} (credential_id, cache_id, scopes) VALUES ($1, $2, $3)
                     ON CONFLICT (credential_id, cache_id) DO UPDATE SET scopes = excluded.scopes"
                ),
                args![
                    credential.id.as_bytes().as_slice(),
                    cache.as_slice(),
                    i64::from(grant.scopes.bits())
                ],
            )?;
        }
        Ok(true)
    }

    fn credential(&self, kind: CredentialKind, id: &CredentialId) -> Result<Option<Credential>> {
        let (table, grants) = tables(kind);
        let found = self.query_opt(
            &format!(
                "SELECT credential_id, tenant_id, principal_id, name, secret_hash,
                        created_at, expires_at, revoked_at
                 FROM {table} WHERE credential_id = $1"
            ),
            args![id.as_bytes().as_slice()],
        )?;
        let Some(row) = found else {
            return Ok(None);
        };
        let mut credential = Credential {
            kind,
            id: CredentialId::from_bytes(row.blob(0)?),
            tenant: TenantId::from_bytes(row.blob(1)?),
            principal: PrincipalId::from_bytes(row.blob(2)?),
            name: row.text(3)?,
            secret_hash: row.blob(4)?,
            created_at: row.int(5)?,
            expires_at: row.int(6)?,
            revoked_at: row.opt_int(7)?,
            grants: Vec::new(),
        };
        for row in self.query(
            &format!(
                "SELECT cache_id, scopes FROM {grants} WHERE credential_id = $1 ORDER BY cache_id"
            ),
            args![id.as_bytes().as_slice()],
        )? {
            credential.grants.push(grant_of(row.blob(0)?, row.int(1)?)?);
        }
        Ok(Some(credential))
    }

    fn revoke_credential(
        &mut self,
        kind: CredentialKind,
        id: &CredentialId,
        now: i64,
    ) -> Result<bool> {
        let (table, _) = tables(kind);
        let changed = self.execute(
            &format!(
                "UPDATE {table} SET revoked_at = $2 WHERE credential_id = $1 AND revoked_at IS NULL"
            ),
            args![id.as_bytes().as_slice(), now],
        )?;
        Ok(changed == 1)
    }

    fn revoke_credentials_of(&mut self, principal: &PrincipalId, now: i64) -> Result<u64> {
        let mut revoked = 0;
        for table in ["tokens", "sessions"] {
            revoked += self.execute(
                &format!(
                    "UPDATE {table} SET revoked_at = $2
                     WHERE principal_id = $1 AND revoked_at IS NULL"
                ),
                args![principal.as_bytes().as_slice(), now],
            )?;
        }
        Ok(revoked)
    }
}
