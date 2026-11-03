//! Workload trust policies.

use crate::domain::oidc::{ClaimCondition, WorkloadPolicy};
use crate::domain::principal::Caller;
use crate::domain::scope::{Grant, Scope, ScopeSet};
use crate::engine::Engine;
use crate::error::{Conflict, Error, Result};
use crate::ids::{CacheId, PrincipalId};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::oidc_rules::check_claim_name;
use std::collections::BTreeMap;

const MAX_NAME_BYTES: usize = 128;
const MAX_CONDITIONS: usize = 32;
const MAX_VALUE_BYTES: usize = 512;

/// A workload policy to create.
#[derive(Debug, Clone)]
pub struct PolicySpec {
    /// The tenant's name for the policy; the natural key.
    pub name: String,
    /// The registered issuer URL tokens must come from.
    pub issuer: String,
    /// Exact claim matches, as claim name and value.
    pub claims: Vec<(String, String)>,
    /// What a matching workload may do, per cache.
    pub grants: Vec<Grant>,
}

fn workload_scopes() -> ScopeSet {
    [Scope::Read, Scope::WritePersonal, Scope::WriteTrusted]
        .into_iter()
        .collect()
}

fn normalised(mut policy: WorkloadPolicy) -> WorkloadPolicy {
    policy.conditions.sort_by(|a, b| a.claim.cmp(&b.claim));
    policy
        .grants
        .sort_by_key(|grant| grant.cache.map(|c| *c.as_bytes()));
    policy
}

fn same_policy(existing: &WorkloadPolicy, wanted: &WorkloadPolicy) -> bool {
    let (existing, wanted) = (normalised(existing.clone()), normalised(wanted.clone()));
    existing.issuer == wanted.issuer
        && existing.conditions == wanted.conditions
        && existing.grants == wanted.grants
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Creates a workload policy and returns its principal and whether it is new. The
    /// identical policy again is not new; a different one of the same name is a
    /// conflict. The policy must match an immutable id claim of its issuer and may
    /// grant `read`, `write:personal` and `write:trusted` only. Needs `tenant:admin`.
    pub async fn create_policy(
        &self,
        caller: &Caller,
        spec: PolicySpec,
    ) -> Result<(PrincipalId, bool)> {
        caller.require_tenant_admin()?;
        let key = self
            .config
            .claim_key
            .clone()
            .ok_or(Error::NotConfigured("the claim hash key"))?;
        if spec.name.is_empty()
            || spec.name.len() > MAX_NAME_BYTES
            || spec.name.chars().any(char::is_control)
        {
            return Err(Error::bad_request(
                "a policy name is 1 to 128 printable characters",
            ));
        }
        if spec.claims.is_empty() || spec.claims.len() > MAX_CONDITIONS {
            return Err(Error::bad_request("a policy matches 1 to 32 claims"));
        }
        let mut values = BTreeMap::new();
        for (claim, value) in spec.claims {
            check_claim_name(&claim)?;
            if value.is_empty() || value.len() > MAX_VALUE_BYTES {
                return Err(Error::bad_request("a claim value is 1 to 512 bytes"));
            }
            if values.insert(claim, value).is_some() {
                return Err(Error::bad_request("a claim is matched once per policy"));
            }
        }
        let mut merged: BTreeMap<[u8; CacheId::LEN], (CacheId, ScopeSet)> = BTreeMap::new();
        for grant in spec.grants {
            let Some(cache) = grant.cache else {
                return Err(Error::bad_request("a workload grant names a cache"));
            };
            if grant.scopes.is_empty() {
                return Err(Error::bad_request("a grant names at least one scope"));
            }
            if [Scope::CacheAdmin, Scope::TenantAdmin]
                .into_iter()
                .any(|scope| grant.scopes.contains(scope))
                || grant.scopes.bits() & !workload_scopes().bits() != 0
            {
                return Err(Error::bad_request(
                    "a workload policy grants read, write:personal and write:trusted only",
                ));
            }
            let entry = merged
                .entry(*cache.as_bytes())
                .or_insert((cache, ScopeSet::EMPTY));
            entry.1 = ScopeSet::from_bits(entry.1.bits() | grant.scopes.bits())
                .ok_or_else(|| Error::internal("scope union"))?;
        }
        if merged.is_empty() {
            return Err(Error::bad_request("a policy grants at least one cache"));
        }
        let tenant = caller.tenant;
        let issuer_url = spec.issuer;
        let name = spec.name;
        let now = self.now();
        self.meta
            .transact(move |tx| {
                let issuer = tx.issuer(&tenant, &issuer_url)?.ok_or_else(|| {
                    Error::bad_request("the issuer is not registered for this tenant")
                })?;
                let names: Vec<&str> = values.keys().map(String::as_str).collect();
                if !issuer.pins_identity(&names) {
                    return Err(Error::bad_request(
                        "a workload policy must match an immutable id claim of its issuer",
                    ));
                }
                let mut grants = Vec::new();
                for (cache, scopes) in merged.into_values() {
                    tx.cache(&tenant, &cache)?.ok_or(Error::NotFound)?;
                    grants.push(Grant {
                        cache: Some(cache),
                        scopes,
                    });
                }
                let conditions = values
                    .iter()
                    .map(|(claim, value)| ClaimCondition {
                        claim: claim.clone(),
                        value_hash: key.hash(&tenant, &issuer_url, claim, value),
                    })
                    .collect();
                let policy = WorkloadPolicy {
                    id: PrincipalId::random(),
                    tenant,
                    issuer: issuer_url,
                    name,
                    conditions,
                    grants,
                    created_at: now,
                };
                if tx.insert_policy(&policy)? {
                    return Ok((policy.id, true));
                }
                let existing = tx
                    .policy_named(&tenant, &policy.name)?
                    .ok_or(Error::NotFound)?;
                if same_policy(&existing, &policy) {
                    Ok((existing.id, false))
                } else {
                    Err(Error::Conflict(Conflict::Exists))
                }
            })
            .await
    }

    /// The tenant's policies; claim values are never stored, so none are returned.
    /// Needs `tenant:admin`.
    pub async fn list_policies(&self, caller: &Caller) -> Result<Vec<WorkloadPolicy>> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        self.meta.transact(move |tx| tx.policies_of(&tenant)).await
    }

    /// Deletes a policy and ends its sessions at once. Needs `tenant:admin`.
    pub async fn remove_policy(&self, caller: &Caller, name: String) -> Result<()> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        let now = self.now();
        self.meta
            .transact(move |tx| {
                let policy = tx.policy_named(&tenant, &name)?.ok_or(Error::NotFound)?;
                tx.revoke_credentials_of(&policy.id, now)?;
                tx.delete_policy(&tenant, &policy.id)?;
                Ok(())
            })
            .await
    }
}
