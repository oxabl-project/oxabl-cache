//! Tenants, caches, users, grants and credentials.

use super::SqliteTx;
use crate::db::{DbResult, RowExt, int};
use oxabl_cache_core::domain::cache::Cache;
use oxabl_cache_core::domain::credential::{Credential, CredentialKind};
use oxabl_cache_core::domain::scope::{Grant, ScopeSet};
use oxabl_cache_core::domain::tenant::{Tenant, TenantStatus};
use oxabl_cache_core::ids::{CacheId, CredentialId, PrincipalId, TenantId};
use oxabl_cache_core::ports::metadata::User;
use oxabl_cache_core::{Error, Result};
use rusqlite::{OptionalExtension, Row, params};

const NO_CACHE: [u8; CacheId::LEN] = [0; CacheId::LEN];

fn cache_of(row: &Row<'_>) -> rusqlite::Result<Cache> {
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

const CACHE_COLUMNS: &str = "cache_id, tenant_id, display_name, retention_days, hard_bytes, \
     epoch, roster_version, live_bytes, pending_bytes, created_at";

fn tables(kind: CredentialKind) -> (&'static str, &'static str) {
    match kind {
        CredentialKind::Token => ("tokens", "token_grants"),
        CredentialKind::Session => ("sessions", "session_grants"),
    }
}

fn grant_of(cache: [u8; CacheId::LEN], scopes: i64) -> Result<Grant> {
    let scopes = u8::try_from(scopes)
        .ok()
        .and_then(ScopeSet::from_bits)
        .ok_or_else(|| Error::internal("stored scopes are malformed"))?;
    Ok(Grant {
        cache: (cache != NO_CACHE).then(|| CacheId::from_bytes(cache)),
        scopes,
    })
}

impl oxabl_cache_core::ports::metadata::AccountsTx for SqliteTx<'_> {
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

    fn insert_user(&mut self, user: &User) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(
                "INSERT INTO users
                   (principal_id, tenant_id, issuer, subject, label, created_at, disabled_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT DO NOTHING",
            )
            .db()?
            .execute(params![
                user.id.as_bytes().as_slice(),
                user.tenant.as_bytes().as_slice(),
                user.issuer,
                user.subject,
                user.label,
                user.created_at,
                user.disabled_at,
            ])
            .db()?;
        Ok(changed == 1)
    }

    fn user(&self, id: &PrincipalId) -> Result<Option<User>> {
        self.conn
            .prepare_cached(
                "SELECT principal_id, tenant_id, issuer, subject, label, created_at, disabled_at
                 FROM users WHERE principal_id = ?1",
            )
            .db()?
            .query_row([id.as_bytes().as_slice()], |row| {
                Ok(User {
                    id: PrincipalId::from_bytes(row.blob(0)?),
                    tenant: TenantId::from_bytes(row.blob(1)?),
                    issuer: row.get(2)?,
                    subject: row.get(3)?,
                    label: row.get(4)?,
                    created_at: row.get(5)?,
                    disabled_at: row.get(6)?,
                })
            })
            .optional()
            .db()
    }

    fn disable_user(&mut self, id: &PrincipalId, now: i64) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                "UPDATE users SET disabled_at = ?2 WHERE principal_id = ?1 AND disabled_at IS NULL",
                params![id.as_bytes().as_slice(), now],
            )
            .db()?;
        Ok(changed == 1)
    }

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

    fn insert_credential(&mut self, credential: &Credential) -> Result<bool> {
        let (table, grants) = tables(credential.kind);
        let changed = self
            .conn
            .prepare_cached(&format!(
                "INSERT INTO {table}
                   (credential_id, tenant_id, principal_id, name, secret_hash,
                    created_at, expires_at, revoked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (credential_id) DO NOTHING"
            ))
            .db()?
            .execute(params![
                credential.id.as_bytes().as_slice(),
                credential.tenant.as_bytes().as_slice(),
                credential.principal.as_bytes().as_slice(),
                credential.name,
                credential.secret_hash.as_slice(),
                credential.created_at,
                credential.expires_at,
                credential.revoked_at,
            ])
            .db()?;
        if changed != 1 {
            return Ok(false);
        }
        for grant in &credential.grants {
            let cache = grant.cache.map_or(NO_CACHE, |cache| *cache.as_bytes());
            self.conn
                .prepare_cached(&format!(
                    "INSERT INTO {grants} (credential_id, cache_id, scopes) VALUES (?1, ?2, ?3)
                     ON CONFLICT (credential_id, cache_id) DO UPDATE SET scopes = excluded.scopes"
                ))
                .db()?
                .execute(params![
                    credential.id.as_bytes().as_slice(),
                    cache.as_slice(),
                    grant.scopes.bits()
                ])
                .db()?;
        }
        Ok(true)
    }

    fn credential(&self, kind: CredentialKind, id: &CredentialId) -> Result<Option<Credential>> {
        let (table, grants) = tables(kind);
        let found = self
            .conn
            .prepare_cached(&format!(
                "SELECT credential_id, tenant_id, principal_id, name, secret_hash,
                        created_at, expires_at, revoked_at
                 FROM {table} WHERE credential_id = ?1"
            ))
            .db()?
            .query_row([id.as_bytes().as_slice()], |row| {
                Ok(Credential {
                    kind,
                    id: CredentialId::from_bytes(row.blob(0)?),
                    tenant: TenantId::from_bytes(row.blob(1)?),
                    principal: PrincipalId::from_bytes(row.blob(2)?),
                    name: row.get(3)?,
                    secret_hash: row.blob(4)?,
                    created_at: row.get(5)?,
                    expires_at: row.get(6)?,
                    revoked_at: row.get(7)?,
                    grants: Vec::new(),
                })
            })
            .optional()
            .db()?;
        let Some(mut credential) = found else {
            return Ok(None);
        };
        let mut statement = self
            .conn
            .prepare_cached(&format!(
                "SELECT cache_id, scopes FROM {grants} WHERE credential_id = ?1 ORDER BY cache_id"
            ))
            .db()?;
        let rows = statement
            .query_map([id.as_bytes().as_slice()], |row| {
                Ok((row.blob::<{ CacheId::LEN }>(0)?, row.get::<_, i64>(1)?))
            })
            .db()?;
        for row in rows {
            let (cache, scopes) = row.db()?;
            credential.grants.push(grant_of(cache, scopes)?);
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
        let changed = self
            .conn
            .execute(
                &format!(
                    "UPDATE {table} SET revoked_at = ?2 WHERE credential_id = ?1 AND revoked_at IS NULL"
                ),
                params![id.as_bytes().as_slice(), now],
            )
            .db()?;
        Ok(changed == 1)
    }

    fn revoke_credentials_of(&mut self, principal: &PrincipalId, now: i64) -> Result<u64> {
        let mut revoked = 0;
        for table in ["tokens", "sessions"] {
            revoked += self
                .conn
                .execute(
                    &format!(
                        "UPDATE {table} SET revoked_at = ?2
                         WHERE principal_id = ?1 AND revoked_at IS NULL"
                    ),
                    params![principal.as_bytes().as_slice(), now],
                )
                .db()?;
        }
        Ok(u64::try_from(revoked).unwrap_or(u64::MAX))
    }
}
