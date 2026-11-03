//! The tenant's issuer allowlist.

use crate::domain::oidc::{Issuer, IssuerShape};
use crate::domain::principal::Caller;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ports::{Clock, IdentityVerifier, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::oidc_rules::check_claim_name;

const MAX_ISSUER_BYTES: usize = 2048;
const MAX_ID_CLAIMS: usize = 8;

/// An issuer to register.
#[derive(Debug, Clone)]
pub struct IssuerSpec {
    /// The issuer URL, exactly as tokens carry it in `iss`.
    pub url: String,
    /// The token shape, which fixes the immutable id claims of GitHub and GitLab.
    pub shape: IssuerShape,
    /// The immutable id claims a custom issuer declares; empty for the other shapes.
    pub id_claims: Vec<String>,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Registers an issuer after the verifier has vetted it, and returns whether it is
    /// new. The identical registration again is not new; a different one is a conflict.
    /// Needs `tenant:admin`.
    pub async fn register_issuer<V: IdentityVerifier>(
        &self,
        caller: &Caller,
        verifier: &V,
        spec: IssuerSpec,
    ) -> Result<bool> {
        caller.require_tenant_admin()?;
        let url = spec.url;
        if url.is_empty()
            || url.len() > MAX_ISSUER_BYTES
            || !url.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(Error::bad_request(
                "the issuer is a URL of printable characters",
            ));
        }
        let mut id_claims = spec.id_claims;
        id_claims.sort_unstable();
        id_claims.dedup();
        match spec.shape {
            IssuerShape::Custom => {
                if id_claims.is_empty() || id_claims.len() > MAX_ID_CLAIMS {
                    return Err(Error::bad_request(
                        "a custom issuer declares 1 to 8 immutable id claims",
                    ));
                }
                id_claims
                    .iter()
                    .try_for_each(|claim| check_claim_name(claim))?;
            }
            IssuerShape::GitHub | IssuerShape::GitLab => {
                if !id_claims.is_empty() {
                    return Err(Error::bad_request(
                        "a GitHub or GitLab issuer has its id claims by default",
                    ));
                }
            }
        }
        let issuer = Issuer {
            tenant: caller.tenant,
            url,
            shape: spec.shape,
            id_claims,
            created_at: self.now(),
        };
        let known = self.stored_issuer(&issuer).await?;
        if let Some(existing) = known {
            return same_issuer(&existing, &issuer).map(|()| false);
        }
        verifier.probe_issuer(&issuer.url).await?;
        self.meta
            .transact(move |tx| {
                if tx.insert_issuer(&issuer)? {
                    return Ok(true);
                }
                let existing = tx
                    .issuer(&issuer.tenant, &issuer.url)?
                    .ok_or(Error::NotFound)?;
                same_issuer(&existing, &issuer).map(|()| false)
            })
            .await
    }

    async fn stored_issuer(&self, issuer: &Issuer) -> Result<Option<Issuer>> {
        let (tenant, url) = (issuer.tenant, issuer.url.clone());
        self.meta.transact(move |tx| tx.issuer(&tenant, &url)).await
    }

    /// The tenant's issuers. Needs `tenant:admin`.
    pub async fn list_issuers(&self, caller: &Caller) -> Result<Vec<Issuer>> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        self.meta.transact(move |tx| tx.issuers_of(&tenant)).await
    }

    /// Removes an issuer with its policies, and ends the sessions of those policies and
    /// of the users who sign in through it. Needs `tenant:admin`.
    pub async fn remove_issuer(&self, caller: &Caller, url: String) -> Result<()> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        let now = self.now();
        self.meta
            .transact(move |tx| {
                tx.issuer(&tenant, &url)?.ok_or(Error::NotFound)?;
                for policy in tx.policies_of(&tenant)? {
                    if policy.issuer == url {
                        tx.revoke_credentials_of(&policy.id, now)?;
                    }
                }
                for user in tx.users_of(&tenant)? {
                    if user.issuer == url {
                        tx.revoke_credentials_of(&user.id, now)?;
                    }
                }
                tx.delete_issuer(&tenant, &url)?;
                Ok(())
            })
            .await
    }
}

fn same_issuer(existing: &Issuer, wanted: &Issuer) -> Result<()> {
    if existing.shape == wanted.shape && existing.id_claims == wanted.id_claims {
        Ok(())
    } else {
        Err(Error::Conflict(crate::error::Conflict::Exists))
    }
}
