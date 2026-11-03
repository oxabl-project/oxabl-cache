//! OIDC issuers and workload trust policies.

use super::SqliteTx;
use crate::db::{DbResult, RowExt};
use oxabl_cache_core::domain::oidc::{ClaimCondition, Issuer, IssuerShape, WorkloadPolicy};
use oxabl_cache_core::domain::scope::{Grant, ScopeSet};
use oxabl_cache_core::ids::{CacheId, PrincipalId, TenantId};
use oxabl_cache_core::ports::metadata::{OidcTx, User};
use oxabl_cache_core::{Error, Result};
use rusqlite::{OptionalExtension, params};

fn shape_code(shape: IssuerShape) -> i64 {
    match shape {
        IssuerShape::GitHub => 0,
        IssuerShape::GitLab => 1,
        IssuerShape::Custom => 2,
    }
}

fn shape_of(code: i64) -> rusqlite::Result<IssuerShape> {
    match code {
        0 => Ok(IssuerShape::GitHub),
        1 => Ok(IssuerShape::GitLab),
        2 => Ok(IssuerShape::Custom),
        _ => Err(rusqlite::Error::IntegralValueOutOfRange(2, code)),
    }
}

impl SqliteTx<'_> {
    fn id_claims(&self, tenant: &TenantId, url: &str) -> Result<Vec<String>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT claim FROM oidc_issuer_claims
                 WHERE tenant_id = ?1 AND issuer = ?2 ORDER BY claim",
            )
            .db()?;
        statement
            .query_map(params![tenant.as_bytes().as_slice(), url], |row| row.get(0))
            .db()?
            .collect::<rusqlite::Result<_>>()
            .db()
    }

    fn policy_parts(&self, mut policy: WorkloadPolicy) -> Result<WorkloadPolicy> {
        let id = policy.id.as_bytes().as_slice();
        let mut claims = self
            .conn
            .prepare_cached(
                "SELECT claim, value_hash FROM workload_policy_claims
                 WHERE policy_id = ?1 ORDER BY claim",
            )
            .db()?;
        policy.conditions = claims
            .query_map([id], |row| {
                Ok(ClaimCondition {
                    claim: row.get(0)?,
                    value_hash: row.blob(1)?,
                })
            })
            .db()?
            .collect::<rusqlite::Result<_>>()
            .db()?;
        let mut grants = self
            .conn
            .prepare_cached(
                "SELECT cache_id, scopes FROM workload_policy_grants
                 WHERE policy_id = ?1 ORDER BY cache_id",
            )
            .db()?;
        let rows = grants
            .query_map([id], |row| {
                Ok((row.blob::<{ CacheId::LEN }>(0)?, row.get::<_, i64>(1)?))
            })
            .db()?;
        for row in rows {
            let (cache, scopes) = row.db()?;
            let scopes = u8::try_from(scopes)
                .ok()
                .and_then(ScopeSet::from_bits)
                .ok_or_else(|| Error::internal("stored scopes are malformed"))?;
            policy.grants.push(Grant {
                cache: Some(CacheId::from_bytes(cache)),
                scopes,
            });
        }
        Ok(policy)
    }

    fn policies(&self, tenant: &TenantId) -> Result<Vec<WorkloadPolicy>> {
        let mut statement = self
            .conn
            .prepare_cached(
                "SELECT policy_id, tenant_id, issuer, name, created_at FROM workload_policies
                 WHERE tenant_id = ?1 ORDER BY created_at, policy_id",
            )
            .db()?;
        let heads: Vec<WorkloadPolicy> = statement
            .query_map([tenant.as_bytes().as_slice()], policy_head)
            .db()?
            .collect::<rusqlite::Result<_>>()
            .db()?;
        heads
            .into_iter()
            .map(|policy| self.policy_parts(policy))
            .collect()
    }
}

fn policy_head(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkloadPolicy> {
    Ok(WorkloadPolicy {
        id: PrincipalId::from_bytes(row.blob(0)?),
        tenant: TenantId::from_bytes(row.blob(1)?),
        issuer: row.get(2)?,
        name: row.get(3)?,
        conditions: Vec::new(),
        grants: Vec::new(),
        created_at: row.get(4)?,
    })
}

impl OidcTx for SqliteTx<'_> {
    fn insert_issuer(&mut self, issuer: &Issuer) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(
                "INSERT INTO oidc_issuers (tenant_id, issuer, shape, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (tenant_id, issuer) DO NOTHING",
            )
            .db()?
            .execute(params![
                issuer.tenant.as_bytes().as_slice(),
                issuer.url,
                shape_code(issuer.shape),
                issuer.created_at,
            ])
            .db()?;
        if changed != 1 {
            return Ok(false);
        }
        for claim in &issuer.id_claims {
            self.conn
                .prepare_cached(
                    "INSERT INTO oidc_issuer_claims (tenant_id, issuer, claim) VALUES (?1, ?2, ?3)
                     ON CONFLICT (tenant_id, issuer, claim) DO NOTHING",
                )
                .db()?
                .execute(params![
                    issuer.tenant.as_bytes().as_slice(),
                    issuer.url,
                    claim
                ])
                .db()?;
        }
        Ok(true)
    }

    fn issuer(&self, tenant: &TenantId, url: &str) -> Result<Option<Issuer>> {
        let found = self
            .conn
            .prepare_cached(
                "SELECT shape, created_at FROM oidc_issuers WHERE tenant_id = ?1 AND issuer = ?2",
            )
            .db()?
            .query_row(params![tenant.as_bytes().as_slice(), url], |row| {
                Ok((shape_of(row.get(0)?)?, row.get::<_, i64>(1)?))
            })
            .optional()
            .db()?;
        let Some((shape, created_at)) = found else {
            return Ok(None);
        };
        Ok(Some(Issuer {
            tenant: *tenant,
            url: url.to_owned(),
            shape,
            id_claims: self.id_claims(tenant, url)?,
            created_at,
        }))
    }

    fn issuers_of(&self, tenant: &TenantId) -> Result<Vec<Issuer>> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT issuer FROM oidc_issuers WHERE tenant_id = ?1 ORDER BY issuer")
            .db()?;
        let urls: Vec<String> = statement
            .query_map([tenant.as_bytes().as_slice()], |row| row.get(0))
            .db()?
            .collect::<rusqlite::Result<_>>()
            .db()?;
        urls.iter()
            .filter_map(|url| self.issuer(tenant, url).transpose())
            .collect()
    }

    fn delete_issuer(&mut self, tenant: &TenantId, url: &str) -> Result<bool> {
        let tenant = tenant.as_bytes().as_slice();
        for statement in [
            "DELETE FROM workload_policy_claims WHERE policy_id IN
               (SELECT policy_id FROM workload_policies WHERE tenant_id = ?1 AND issuer = ?2)",
            "DELETE FROM workload_policy_grants WHERE policy_id IN
               (SELECT policy_id FROM workload_policies WHERE tenant_id = ?1 AND issuer = ?2)",
            "DELETE FROM workload_policies WHERE tenant_id = ?1 AND issuer = ?2",
            "DELETE FROM oidc_issuer_claims WHERE tenant_id = ?1 AND issuer = ?2",
        ] {
            self.conn.execute(statement, params![tenant, url]).db()?;
        }
        let changed = self
            .conn
            .execute(
                "DELETE FROM oidc_issuers WHERE tenant_id = ?1 AND issuer = ?2",
                params![tenant, url],
            )
            .db()?;
        Ok(changed == 1)
    }

    fn insert_policy(&mut self, policy: &WorkloadPolicy) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(
                "INSERT INTO workload_policies (policy_id, tenant_id, issuer, name, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (tenant_id, name) DO NOTHING",
            )
            .db()?
            .execute(params![
                policy.id.as_bytes().as_slice(),
                policy.tenant.as_bytes().as_slice(),
                policy.issuer,
                policy.name,
                policy.created_at,
            ])
            .db()?;
        if changed != 1 {
            return Ok(false);
        }
        for condition in &policy.conditions {
            self.conn
                .prepare_cached(
                    "INSERT INTO workload_policy_claims (policy_id, claim, value_hash)
                     VALUES (?1, ?2, ?3)",
                )
                .db()?
                .execute(params![
                    policy.id.as_bytes().as_slice(),
                    condition.claim,
                    condition.value_hash.as_slice(),
                ])
                .db()?;
        }
        for grant in &policy.grants {
            let cache = grant
                .cache
                .ok_or_else(|| Error::internal("a workload grant names a cache"))?;
            self.conn
                .prepare_cached(
                    "INSERT INTO workload_policy_grants (policy_id, cache_id, scopes)
                     VALUES (?1, ?2, ?3)",
                )
                .db()?
                .execute(params![
                    policy.id.as_bytes().as_slice(),
                    cache.as_bytes().as_slice(),
                    grant.scopes.bits(),
                ])
                .db()?;
        }
        Ok(true)
    }

    fn policy_named(&self, tenant: &TenantId, name: &str) -> Result<Option<WorkloadPolicy>> {
        let found = self
            .conn
            .prepare_cached(
                "SELECT policy_id, tenant_id, issuer, name, created_at FROM workload_policies
                 WHERE tenant_id = ?1 AND name = ?2",
            )
            .db()?
            .query_row(params![tenant.as_bytes().as_slice(), name], policy_head)
            .optional()
            .db()?;
        found.map(|policy| self.policy_parts(policy)).transpose()
    }

    fn policies_of(&self, tenant: &TenantId) -> Result<Vec<WorkloadPolicy>> {
        self.policies(tenant)
    }

    fn delete_policy(&mut self, tenant: &TenantId, id: &PrincipalId) -> Result<bool> {
        let owned: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM workload_policies WHERE policy_id = ?1 AND tenant_id = ?2",
                params![id.as_bytes().as_slice(), tenant.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .optional()
            .db()?;
        if owned.is_none() {
            return Ok(false);
        }
        for table in ["workload_policy_claims", "workload_policy_grants"] {
            self.conn
                .execute(
                    &format!("DELETE FROM {table} WHERE policy_id = ?1"),
                    [id.as_bytes().as_slice()],
                )
                .db()?;
        }
        self.conn
            .execute(
                "DELETE FROM workload_policies WHERE policy_id = ?1",
                [id.as_bytes().as_slice()],
            )
            .db()?;
        Ok(true)
    }

    fn user_by_subject(
        &self,
        tenant: &TenantId,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<User>> {
        self.conn
            .prepare_cached(
                "SELECT principal_id, tenant_id, issuer, subject, label, created_at, disabled_at
                 FROM users WHERE tenant_id = ?1 AND issuer = ?2 AND subject = ?3",
            )
            .db()?
            .query_row(
                params![tenant.as_bytes().as_slice(), issuer, subject],
                |row| {
                    Ok(User {
                        id: PrincipalId::from_bytes(row.blob(0)?),
                        tenant: TenantId::from_bytes(row.blob(1)?),
                        issuer: row.get(2)?,
                        subject: row.get(3)?,
                        label: row.get(4)?,
                        created_at: row.get(5)?,
                        disabled_at: row.get(6)?,
                    })
                },
            )
            .optional()
            .db()
    }
}
