//! OIDC issuers and workload trust policies.

use super::accounts::{USER_COLUMNS, user_of};
use super::{PgTx, RowExt, args};
use oxabl_cache_core::domain::oidc::{ClaimCondition, Issuer, IssuerShape, WorkloadPolicy};
use oxabl_cache_core::domain::scope::{Grant, ScopeSet};
use oxabl_cache_core::ids::{CacheId, PrincipalId, TenantId};
use oxabl_cache_core::ports::metadata::{OidcTx, User};
use oxabl_cache_core::{Error, Result};
use tokio_postgres::Row;

fn shape_code(shape: IssuerShape) -> i64 {
    match shape {
        IssuerShape::GitHub => 0,
        IssuerShape::GitLab => 1,
        IssuerShape::Custom => 2,
    }
}

fn shape_of(code: i64) -> Result<IssuerShape> {
    match code {
        0 => Ok(IssuerShape::GitHub),
        1 => Ok(IssuerShape::GitLab),
        2 => Ok(IssuerShape::Custom),
        _ => Err(Error::internal("stored issuer shape is unknown")),
    }
}

fn policy_head(row: &Row) -> Result<WorkloadPolicy> {
    Ok(WorkloadPolicy {
        id: PrincipalId::from_bytes(row.blob(0)?),
        tenant: TenantId::from_bytes(row.blob(1)?),
        issuer: row.text(2)?,
        name: row.text(3)?,
        conditions: Vec::new(),
        grants: Vec::new(),
        created_at: row.int(4)?,
    })
}

impl PgTx<'_> {
    fn id_claims(&self, tenant: &TenantId, url: &str) -> Result<Vec<String>> {
        self.query(
            "SELECT claim FROM oidc_issuer_claims
             WHERE tenant_id = $1 AND issuer = $2 ORDER BY claim",
            args![tenant.as_bytes().as_slice(), url],
        )?
        .iter()
        .map(|row| row.text(0))
        .collect()
    }

    fn policy_parts(&self, mut policy: WorkloadPolicy) -> Result<WorkloadPolicy> {
        let id = policy.id.as_bytes().as_slice();
        policy.conditions = self
            .query(
                "SELECT claim, value_hash FROM workload_policy_claims
                 WHERE policy_id = $1 ORDER BY claim",
                args![id],
            )?
            .iter()
            .map(|row| {
                Ok(ClaimCondition {
                    claim: row.text(0)?,
                    value_hash: row.blob(1)?,
                })
            })
            .collect::<Result<_>>()?;
        for row in self.query(
            "SELECT cache_id, scopes FROM workload_policy_grants
             WHERE policy_id = $1 ORDER BY cache_id",
            args![id],
        )? {
            let scopes = u8::try_from(row.int(1)?)
                .ok()
                .and_then(ScopeSet::from_bits)
                .ok_or_else(|| Error::internal("stored scopes are malformed"))?;
            policy.grants.push(Grant {
                cache: Some(CacheId::from_bytes(row.blob(0)?)),
                scopes,
            });
        }
        Ok(policy)
    }
}

impl OidcTx for PgTx<'_> {
    fn insert_issuer(&mut self, issuer: &Issuer) -> Result<bool> {
        let changed = self.execute(
            "INSERT INTO oidc_issuers (tenant_id, issuer, shape, created_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (tenant_id, issuer) DO NOTHING",
            args![
                issuer.tenant.as_bytes().as_slice(),
                issuer.url,
                shape_code(issuer.shape),
                issuer.created_at,
            ],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        for claim in &issuer.id_claims {
            self.execute(
                "INSERT INTO oidc_issuer_claims (tenant_id, issuer, claim) VALUES ($1, $2, $3)
                 ON CONFLICT (tenant_id, issuer, claim) DO NOTHING",
                args![issuer.tenant.as_bytes().as_slice(), issuer.url, claim],
            )?;
        }
        Ok(true)
    }

    fn issuer(&self, tenant: &TenantId, url: &str) -> Result<Option<Issuer>> {
        let Some(row) = self.query_opt(
            "SELECT shape, created_at FROM oidc_issuers WHERE tenant_id = $1 AND issuer = $2",
            args![tenant.as_bytes().as_slice(), url],
        )?
        else {
            return Ok(None);
        };
        Ok(Some(Issuer {
            tenant: *tenant,
            url: url.to_owned(),
            shape: shape_of(row.int(0)?)?,
            id_claims: self.id_claims(tenant, url)?,
            created_at: row.int(1)?,
        }))
    }

    fn issuers_of(&self, tenant: &TenantId) -> Result<Vec<Issuer>> {
        let urls = self
            .query(
                "SELECT issuer FROM oidc_issuers WHERE tenant_id = $1 ORDER BY issuer",
                args![tenant.as_bytes().as_slice()],
            )?
            .iter()
            .map(|row| row.text(0))
            .collect::<Result<Vec<_>>>()?;
        urls.iter()
            .filter_map(|url| self.issuer(tenant, url).transpose())
            .collect()
    }

    fn delete_issuer(&mut self, tenant: &TenantId, url: &str) -> Result<bool> {
        let tenant = tenant.as_bytes().as_slice();
        for statement in [
            "DELETE FROM workload_policy_claims WHERE policy_id IN
               (SELECT policy_id FROM workload_policies WHERE tenant_id = $1 AND issuer = $2)",
            "DELETE FROM workload_policy_grants WHERE policy_id IN
               (SELECT policy_id FROM workload_policies WHERE tenant_id = $1 AND issuer = $2)",
            "DELETE FROM workload_policies WHERE tenant_id = $1 AND issuer = $2",
            "DELETE FROM oidc_issuer_claims WHERE tenant_id = $1 AND issuer = $2",
        ] {
            self.execute(statement, args![tenant, url])?;
        }
        let changed = self.execute(
            "DELETE FROM oidc_issuers WHERE tenant_id = $1 AND issuer = $2",
            args![tenant, url],
        )?;
        Ok(changed == 1)
    }

    fn insert_policy(&mut self, policy: &WorkloadPolicy) -> Result<bool> {
        let changed = self.execute(
            "INSERT INTO workload_policies (policy_id, tenant_id, issuer, name, created_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (tenant_id, name) DO NOTHING",
            args![
                policy.id.as_bytes().as_slice(),
                policy.tenant.as_bytes().as_slice(),
                policy.issuer,
                policy.name,
                policy.created_at,
            ],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        for condition in &policy.conditions {
            self.execute(
                "INSERT INTO workload_policy_claims (policy_id, claim, value_hash)
                 VALUES ($1, $2, $3)",
                args![
                    policy.id.as_bytes().as_slice(),
                    condition.claim,
                    condition.value_hash.as_slice(),
                ],
            )?;
        }
        for grant in &policy.grants {
            let cache = grant
                .cache
                .ok_or_else(|| Error::internal("a workload grant names a cache"))?;
            self.execute(
                "INSERT INTO workload_policy_grants (policy_id, cache_id, scopes)
                 VALUES ($1, $2, $3)",
                args![
                    policy.id.as_bytes().as_slice(),
                    cache.as_bytes().as_slice(),
                    i64::from(grant.scopes.bits()),
                ],
            )?;
        }
        Ok(true)
    }

    fn policy_named(&self, tenant: &TenantId, name: &str) -> Result<Option<WorkloadPolicy>> {
        self.query_opt(
            "SELECT policy_id, tenant_id, issuer, name, created_at FROM workload_policies
             WHERE tenant_id = $1 AND name = $2",
            args![tenant.as_bytes().as_slice(), name],
        )?
        .as_ref()
        .map(policy_head)
        .transpose()?
        .map(|policy| self.policy_parts(policy))
        .transpose()
    }

    fn policies_of(&self, tenant: &TenantId) -> Result<Vec<WorkloadPolicy>> {
        self.query(
            "SELECT policy_id, tenant_id, issuer, name, created_at FROM workload_policies
             WHERE tenant_id = $1 ORDER BY created_at, policy_id",
            args![tenant.as_bytes().as_slice()],
        )?
        .iter()
        .map(|row| self.policy_parts(policy_head(row)?))
        .collect()
    }

    fn delete_policy(&mut self, tenant: &TenantId, id: &PrincipalId) -> Result<bool> {
        let owned = self.query_opt(
            "SELECT 1 FROM workload_policies WHERE policy_id = $1 AND tenant_id = $2",
            args![id.as_bytes().as_slice(), tenant.as_bytes().as_slice()],
        )?;
        if owned.is_none() {
            return Ok(false);
        }
        for table in ["workload_policy_claims", "workload_policy_grants"] {
            self.execute(
                &format!("DELETE FROM {table} WHERE policy_id = $1"),
                args![id.as_bytes().as_slice()],
            )?;
        }
        self.execute(
            "DELETE FROM workload_policies WHERE policy_id = $1",
            args![id.as_bytes().as_slice()],
        )?;
        Ok(true)
    }

    fn user_by_subject(
        &self,
        tenant: &TenantId,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<User>> {
        self.query_opt(
            &format!(
                "SELECT {USER_COLUMNS} FROM users
                 WHERE tenant_id = $1 AND issuer = $2 AND subject = $3"
            ),
            args![tenant.as_bytes().as_slice(), issuer, subject],
        )?
        .as_ref()
        .map(user_of)
        .transpose()
    }
}
