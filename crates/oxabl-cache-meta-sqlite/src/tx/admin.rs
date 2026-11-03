//! Listings and operator-level writes.

use super::SqliteTx;
use crate::db::{DbResult, RowExt, int};
use oxabl_cache_core::domain::cache::Cache;
use oxabl_cache_core::domain::credential::{OperatorToken, TokenSummary};
use oxabl_cache_core::domain::scope::{Grant, ScopeSet};
use oxabl_cache_core::domain::tenant::{Tenant, TenantLimits, TenantStatus};
use oxabl_cache_core::ids::{CacheId, CredentialId, PrincipalId, TenantId};
use oxabl_cache_core::ports::metadata::{AdminTx, User};
use oxabl_cache_core::{Error, Result};
use rusqlite::{OptionalExtension, params};

const NO_CACHE: [u8; CacheId::LEN] = [0; CacheId::LEN];

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

impl AdminTx for SqliteTx<'_> {
    fn all_tenants(&self) -> Result<Vec<Tenant>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT tenant_id, name, status, hard_bytes, egress_bytes, rate, burst, created_at
                 FROM tenants ORDER BY tenant_id",
            )
            .db()?;
        let rows = statement
            .query_map([], |row| {
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
            .db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }

    fn set_tenant_limits(&mut self, id: &TenantId, limits: &TenantLimits) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(
                "UPDATE tenants SET hard_bytes = ?2, egress_bytes = ?3, rate = ?4, burst = ?5
                 WHERE tenant_id = ?1",
            )
            .db()?
            .execute(params![
                id.as_bytes().as_slice(),
                limits.hard_bytes.map(int),
                limits.egress_bytes.map(int),
                limits.rate,
                limits.burst,
            ])
            .db()?;
        Ok(changed == 1)
    }

    fn caches_of(&self, tenant: &TenantId) -> Result<Vec<Cache>> {
        Ok(
            oxabl_cache_core::ports::metadata::CachesTx::all_caches(self)?
                .into_iter()
                .filter(|cache| cache.tenant == *tenant)
                .collect(),
        )
    }

    fn tokens_of(&self, tenant: &TenantId) -> Result<Vec<TokenSummary>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT credential_id, principal_id, name, created_at, expires_at, revoked_at
                 FROM tokens WHERE tenant_id = ?1 ORDER BY created_at, credential_id",
            )
            .db()?;
        let rows = statement
            .query_map([tenant.as_bytes().as_slice()], |row| {
                Ok(TokenSummary {
                    id: CredentialId::from_bytes(row.blob(0)?),
                    principal: PrincipalId::from_bytes(row.blob(1)?),
                    name: row.get(2)?,
                    created_at: row.get(3)?,
                    expires_at: row.get(4)?,
                    revoked_at: row.get(5)?,
                    grants: Vec::new(),
                })
            })
            .db()?;
        let mut tokens: Vec<TokenSummary> = rows.collect::<rusqlite::Result<_>>().db()?;
        for token in &mut tokens {
            let mut statement = self
                .conn
                .prepare_cached(
                    "SELECT cache_id, scopes FROM token_grants
                     WHERE credential_id = ?1 ORDER BY cache_id",
                )
                .db()?;
            let rows = statement
                .query_map([token.id.as_bytes().as_slice()], |row| {
                    Ok((row.blob::<{ CacheId::LEN }>(0)?, row.get::<_, i64>(1)?))
                })
                .db()?;
            for row in rows {
                let (cache, scopes) = row.db()?;
                token.grants.push(grant_of(cache, scopes)?);
            }
        }
        Ok(tokens)
    }

    fn users_of(&self, tenant: &TenantId) -> Result<Vec<User>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT principal_id, tenant_id, issuer, subject, label, created_at, disabled_at
                 FROM users WHERE tenant_id = ?1 ORDER BY created_at, principal_id",
            )
            .db()?;
        let rows = statement
            .query_map([tenant.as_bytes().as_slice()], |row| {
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
            .db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }

    fn tenant_grants(&self, tenant: &TenantId) -> Result<Vec<(PrincipalId, Grant)>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT principal_id, cache_id, scopes FROM grants
                 WHERE tenant_id = ?1 ORDER BY principal_id, cache_id",
            )
            .db()?;
        let rows = statement
            .query_map([tenant.as_bytes().as_slice()], |row| {
                Ok((
                    PrincipalId::from_bytes(row.blob(0)?),
                    row.blob::<{ CacheId::LEN }>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .db()?;
        rows.map(|row| {
            let (principal, cache, scopes) = row.db()?;
            Ok((principal, grant_of(cache, scopes)?))
        })
        .collect()
    }

    fn insert_operator_token(&mut self, token: &OperatorToken) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(
                "INSERT INTO operator_tokens
                   (credential_id, name, secret_hash, created_at, expires_at, revoked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (credential_id) DO NOTHING",
            )
            .db()?
            .execute(params![
                token.id.as_bytes().as_slice(),
                token.name,
                token.secret_hash.as_slice(),
                token.created_at,
                token.expires_at,
                token.revoked_at,
            ])
            .db()?;
        Ok(changed == 1)
    }

    fn operator_token(&self, id: &CredentialId) -> Result<Option<OperatorToken>> {
        self.conn
            .prepare_cached(
                "SELECT credential_id, name, secret_hash, created_at, expires_at, revoked_at
                 FROM operator_tokens WHERE credential_id = ?1",
            )
            .db()?
            .query_row([id.as_bytes().as_slice()], |row| {
                Ok(OperatorToken {
                    id: CredentialId::from_bytes(row.blob(0)?),
                    name: row.get(1)?,
                    secret_hash: row.blob(2)?,
                    created_at: row.get(3)?,
                    expires_at: row.get(4)?,
                    revoked_at: row.get(5)?,
                })
            })
            .optional()
            .db()
    }

    fn operator_tokens_exist(&self) -> Result<bool> {
        self.conn
            .prepare_cached("SELECT EXISTS (SELECT 1 FROM operator_tokens)")
            .db()?
            .query_row([], |row| row.get::<_, i64>(0))
            .map(|found| found == 1)
            .db()
    }
}
