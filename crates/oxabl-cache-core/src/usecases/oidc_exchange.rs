//! Exchanging a verified identity token for a session.

use crate::domain::oidc::WorkloadPolicy;
use crate::domain::scope::{Grant, ScopeSet};
use crate::domain::tenant::TenantStatus;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::{CacheId, TenantId};
use crate::ports::{
    Clock, IdentityVerifier, MetadataStore, ObjectStore, PackAssembler, VerifiedIdentity,
};
use crate::usecases::token_create::NewCredential;
use subtle::ConstantTimeEq;

/// A session opened by an identity token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchanged {
    /// The session credential, shown once.
    pub session: NewCredential,
    /// What the session may do.
    pub grants: Vec<Grant>,
}

enum Resolved {
    Workload(crate::ids::PrincipalId, Vec<Grant>),
    User(crate::ids::PrincipalId),
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Verifies an identity token for a tenant and opens a session: a workload
    /// session when trust policies match, a user session when the token's issuer and
    /// subject name a registered user. Anything else is `Unauthenticated`.
    pub async fn exchange_identity<V: IdentityVerifier>(
        &self,
        verifier: &V,
        tenant: TenantId,
        id_token: &str,
    ) -> Result<Exchanged> {
        let (status, issuers) = self
            .meta
            .transact(move |tx| {
                let status = tx.tenant(&tenant)?.map(|found| found.status);
                let issuers = tx.issuers_of(&tenant)?;
                Ok((status, issuers))
            })
            .await?;
        match status {
            None => return Err(Error::Unauthenticated),
            Some(TenantStatus::Suspended) => return Err(Error::TenantSuspended),
            Some(TenantStatus::Active) => {}
        }
        if issuers.is_empty() {
            return Err(Error::Unauthenticated);
        }
        let urls: Vec<String> = issuers.into_iter().map(|issuer| issuer.url).collect();
        let identity = verifier.verify(&tenant, &urls, id_token).await?;
        let key = self.config.claim_key.clone();
        let resolved = self
            .meta
            .transact(move |tx| {
                let matched: Vec<WorkloadPolicy> = match &key {
                    Some(key) => tx
                        .policies_of(&tenant)?
                        .into_iter()
                        .filter(|policy| policy_matches(policy, &identity, key, &tenant))
                        .collect(),
                    None => Vec::new(),
                };
                if let Some(first) = matched.first() {
                    let grants = union_grants(&matched);
                    return Ok(Resolved::Workload(first.id, grants));
                }
                match tx.user_by_subject(&tenant, &identity.issuer, &identity.subject)? {
                    Some(user) if user.disabled_at.is_none() => Ok(Resolved::User(user.id)),
                    _ => Err(Error::Unauthenticated),
                }
            })
            .await?;
        match resolved {
            Resolved::Workload(principal, grants) => {
                let session = self
                    .open_workload_session(tenant, principal, grants.clone())
                    .await?;
                Ok(Exchanged { session, grants })
            }
            Resolved::User(user) => {
                let (session, grants) = self.user_session(tenant, user).await?;
                Ok(Exchanged { session, grants })
            }
        }
    }
}

fn policy_matches(
    policy: &WorkloadPolicy,
    identity: &VerifiedIdentity,
    key: &crate::domain::claim_key::ClaimKey,
    tenant: &TenantId,
) -> bool {
    policy.issuer == identity.issuer
        && !policy.conditions.is_empty()
        && policy.conditions.iter().all(|condition| {
            identity
                .claims
                .iter()
                .filter(|(name, _)| *name == condition.claim)
                .any(|(_, value)| {
                    let hash = key.hash(tenant, &policy.issuer, &condition.claim, value);
                    bool::from(hash.ct_eq(&condition.value_hash))
                })
        })
}

fn union_grants(policies: &[WorkloadPolicy]) -> Vec<Grant> {
    let mut merged: Vec<(CacheId, ScopeSet)> = Vec::new();
    for grant in policies.iter().flat_map(|policy| &policy.grants) {
        let Some(cache) = grant.cache else { continue };
        match merged.iter_mut().find(|(id, _)| *id == cache) {
            Some((_, scopes)) => {
                *scopes =
                    ScopeSet::from_bits(scopes.bits() | grant.scopes.bits()).unwrap_or(*scopes);
            }
            None => merged.push((cache, grant.scopes)),
        }
    }
    merged
        .into_iter()
        .map(|(cache, scopes)| Grant {
            cache: Some(cache),
            scopes,
        })
        .collect()
}
