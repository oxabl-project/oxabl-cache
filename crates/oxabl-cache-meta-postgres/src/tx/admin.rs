//! Listings and operator-level writes.

use super::accounts::{
    CACHE_COLUMNS, TENANT_COLUMNS, USER_COLUMNS, cache_of, grant_of, tenant_of, user_of,
};
use super::{PgTx, RowExt, args, int};
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::cache::Cache;
use oxabl_cache_core::domain::credential::{OperatorToken, TokenSummary};
use oxabl_cache_core::domain::scope::Grant;
use oxabl_cache_core::domain::tenant::{Tenant, TenantLimits};
use oxabl_cache_core::ids::{CredentialId, PrincipalId, TenantId};
use oxabl_cache_core::ports::metadata::{AdminTx, User};

impl AdminTx for PgTx<'_> {
    fn all_tenants(&self) -> Result<Vec<Tenant>> {
        self.query(
            &format!(
                "SELECT {TENANT_COLUMNS} FROM tenants WHERE deleted_at IS NULL ORDER BY tenant_id"
            ),
            args![],
        )?
        .iter()
        .map(tenant_of)
        .collect()
    }

    fn set_tenant_limits(&mut self, id: &TenantId, limits: &TenantLimits) -> Result<bool> {
        let changed = self.execute(
            "UPDATE tenants SET hard_bytes = $2, egress_bytes = $3, rate = $4, burst = $5
             WHERE tenant_id = $1 AND deleted_at IS NULL",
            args![
                id.as_bytes().as_slice(),
                limits.hard_bytes.map(int),
                limits.egress_bytes.map(int),
                i64::from(limits.rate),
                i64::from(limits.burst),
            ],
        )?;
        Ok(changed == 1)
    }

    fn caches_of(&self, tenant: &TenantId) -> Result<Vec<Cache>> {
        self.query(
            &format!(
                "SELECT {CACHE_COLUMNS} FROM caches
                 WHERE tenant_id = $1 AND deleted_at IS NULL ORDER BY cache_id"
            ),
            args![tenant.as_bytes().as_slice()],
        )?
        .iter()
        .map(cache_of)
        .collect()
    }

    fn tokens_of(&self, tenant: &TenantId) -> Result<Vec<TokenSummary>> {
        let mut tokens = Vec::new();
        for row in self.query(
            "SELECT credential_id, principal_id, name, created_at, expires_at, revoked_at
             FROM tokens WHERE tenant_id = $1 ORDER BY created_at, credential_id",
            args![tenant.as_bytes().as_slice()],
        )? {
            tokens.push(TokenSummary {
                id: CredentialId::from_bytes(row.blob(0)?),
                principal: PrincipalId::from_bytes(row.blob(1)?),
                name: row.text(2)?,
                created_at: row.int(3)?,
                expires_at: row.int(4)?,
                revoked_at: row.opt_int(5)?,
                grants: Vec::new(),
            });
        }
        for token in &mut tokens {
            for row in self.query(
                "SELECT cache_id, scopes FROM token_grants
                 WHERE credential_id = $1 ORDER BY cache_id",
                args![token.id.as_bytes().as_slice()],
            )? {
                token.grants.push(grant_of(row.blob(0)?, row.int(1)?)?);
            }
        }
        Ok(tokens)
    }

    fn users_of(&self, tenant: &TenantId) -> Result<Vec<User>> {
        self.query(
            &format!(
                "SELECT {USER_COLUMNS} FROM users WHERE tenant_id = $1
                 ORDER BY created_at, principal_id"
            ),
            args![tenant.as_bytes().as_slice()],
        )?
        .iter()
        .map(user_of)
        .collect()
    }

    fn tenant_grants(&self, tenant: &TenantId) -> Result<Vec<(PrincipalId, Grant)>> {
        self.query(
            "SELECT principal_id, cache_id, scopes FROM grants
             WHERE tenant_id = $1 ORDER BY principal_id, cache_id",
            args![tenant.as_bytes().as_slice()],
        )?
        .iter()
        .map(|row| {
            Ok((
                PrincipalId::from_bytes(row.blob(0)?),
                grant_of(row.blob(1)?, row.int(2)?)?,
            ))
        })
        .collect()
    }

    fn insert_operator_token(&mut self, token: &OperatorToken) -> Result<bool> {
        let changed = self.execute(
            "INSERT INTO operator_tokens
               (credential_id, name, secret_hash, created_at, expires_at, revoked_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (credential_id) DO NOTHING",
            args![
                token.id.as_bytes().as_slice(),
                token.name,
                token.secret_hash.as_slice(),
                token.created_at,
                token.expires_at,
                token.revoked_at,
            ],
        )?;
        Ok(changed == 1)
    }

    fn operator_token(&self, id: &CredentialId) -> Result<Option<OperatorToken>> {
        self.query_opt(
            "SELECT credential_id, name, secret_hash, created_at, expires_at, revoked_at
             FROM operator_tokens WHERE credential_id = $1",
            args![id.as_bytes().as_slice()],
        )?
        .map(|row| {
            Ok(OperatorToken {
                id: CredentialId::from_bytes(row.blob(0)?),
                name: row.text(1)?,
                secret_hash: row.blob(2)?,
                created_at: row.int(3)?,
                expires_at: row.int(4)?,
                revoked_at: row.opt_int(5)?,
            })
        })
        .transpose()
    }

    fn operator_tokens_exist(&self) -> Result<bool> {
        self.query_one("SELECT EXISTS (SELECT 1 FROM operator_tokens)", args![])?
            .flag(0)
    }
}
