//! The first human administrator of a tenant, named by an operator.

use crate::domain::oidc::{Issuer, IssuerShape};
use crate::domain::scope::{Grant, Scope, ScopeSet};
use crate::domain::tenant::TenantStatus;
use crate::engine::Engine;
use crate::error::{Conflict, Error, Result};
use crate::ids::{PrincipalId, TenantId};
use crate::ports::metadata::User;
use crate::ports::{Clock, IdentityVerifier, MetadataStore, ObjectStore, PackAssembler};

const SUBJECT_CLAIM: &str = "sub";

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Binds a human `tenant:admin` principal to an issuer and subject, registering the
    /// issuer for the tenant when it is not yet (after the verifier vetted it), and returns
    /// the principal and whether it is new. Doing it again for the same subject returns the
    /// same principal. For operators; the caller authenticated as one.
    pub async fn bootstrap_tenant_admin<V: IdentityVerifier>(
        &self,
        verifier: &V,
        tenant: TenantId,
        issuer: String,
        subject: String,
    ) -> Result<(PrincipalId, bool)> {
        if subject.is_empty() || subject.len() > 512 || subject.chars().any(char::is_control) {
            return Err(Error::bad_request(
                "a subject is 1 to 512 printable characters",
            ));
        }
        if issuer.is_empty() || issuer.len() > 2048 || !issuer.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(Error::bad_request(
                "the issuer is a URL of printable characters",
            ));
        }
        let (url, known) = (issuer.clone(), tenant);
        let registered = self
            .meta
            .transact(move |tx| {
                match tx.tenant(&known)?.map(|found| found.status) {
                    None => return Err(Error::NotFound),
                    Some(TenantStatus::Suspended) => return Err(Error::TenantSuspended),
                    Some(TenantStatus::Active) => {}
                }
                tx.issuer(&known, &url)
            })
            .await?;
        if registered.is_none() {
            verifier.probe_issuer(&issuer).await?;
        }
        let now = self.now();
        self.meta
            .transact(move |tx| {
                if tx.issuer(&tenant, &issuer)?.is_none() {
                    tx.insert_issuer(&Issuer {
                        tenant,
                        url: issuer.clone(),
                        shape: IssuerShape::Custom,
                        id_claims: vec![SUBJECT_CLAIM.to_owned()],
                        created_at: now,
                    })?;
                }
                if let Some(existing) = tx.user_by_subject(&tenant, &issuer, &subject)? {
                    let admin = tx.grants_of(&existing.id)?.iter().any(|grant| {
                        grant.cache.is_none() && grant.scopes.contains(Scope::TenantAdmin)
                    });
                    return if admin && existing.disabled_at.is_none() {
                        Ok((existing.id, false))
                    } else {
                        Err(Error::Conflict(Conflict::Exists))
                    };
                }
                let user = User {
                    id: PrincipalId::random(),
                    tenant,
                    issuer,
                    subject,
                    label: "tenant administrator".to_owned(),
                    created_at: now,
                    disabled_at: None,
                };
                if !tx.insert_user(&user)? {
                    return Err(Error::Conflict(Conflict::Exists));
                }
                tx.set_grant(
                    &tenant,
                    &user.id,
                    &Grant {
                        cache: None,
                        scopes: ScopeSet::EMPTY.with(Scope::TenantAdmin),
                    },
                )?;
                Ok((user.id, true))
            })
            .await
    }
}
