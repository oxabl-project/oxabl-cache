//! Opening sessions after an identity has been verified.

use crate::domain::credential::{Credential, CredentialKind};
use crate::domain::scope::{Grant, Scope};
use crate::engine::Engine;
use crate::error::{Conflict, Error, Result};
use crate::ids::{PrincipalId, TenantId};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::credential_text::{generate, hash_secret};
use crate::usecases::token_create::NewCredential;

const WORKLOAD_SESSION_SECS: i64 = 60 * 60;
const USER_SESSION_SECS: i64 = 12 * 60 * 60;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Opens a one-hour session for a workload with the grants its trust policy
    /// gave. A workload never receives an administration scope.
    pub async fn open_workload_session(
        &self,
        tenant: TenantId,
        principal: PrincipalId,
        grants: Vec<Grant>,
    ) -> Result<NewCredential> {
        let admin = grants
            .iter()
            .any(|g| g.scopes.contains(Scope::CacheAdmin) || g.scopes.contains(Scope::TenantAdmin));
        if admin {
            return Err(Error::Forbidden(
                "a workload never receives an administration scope",
            ));
        }
        self.open_session(tenant, principal, grants, WORKLOAD_SESSION_SECS, "workload")
            .await
    }

    /// Opens a twelve-hour session for a user with the user's standing grants.
    pub async fn open_user_session(
        &self,
        tenant: TenantId,
        user: PrincipalId,
    ) -> Result<NewCredential> {
        let grants = self
            .meta
            .transact(move |tx| match tx.user(&user)? {
                Some(found) if found.tenant == tenant && found.disabled_at.is_none() => {
                    tx.grants_of(&user)
                }
                _ => Err(Error::NotFound),
            })
            .await?;
        self.open_session(tenant, user, grants, USER_SESSION_SECS, "user")
            .await
    }

    async fn open_session(
        &self,
        tenant: TenantId,
        principal: PrincipalId,
        grants: Vec<Grant>,
        lifetime: i64,
        name: &str,
    ) -> Result<NewCredential> {
        let now = self.now();
        let (id, secret, text) = generate(CredentialKind::Session);
        let expires_at = now + lifetime;
        let name = name.to_owned();
        self.meta
            .transact(move |tx| {
                for grant in &grants {
                    if let Some(cache) = &grant.cache {
                        tx.cache(&tenant, cache)?.ok_or(Error::NotFound)?;
                    }
                }
                let session = Credential {
                    kind: CredentialKind::Session,
                    id,
                    tenant,
                    principal,
                    name,
                    secret_hash: hash_secret(&secret),
                    created_at: now,
                    expires_at,
                    revoked_at: None,
                    grants,
                };
                if !tx.insert_credential(&session)? {
                    return Err(Error::Conflict(Conflict::Exists));
                }
                Ok(())
            })
            .await?;
        Ok(NewCredential {
            secret: text,
            id,
            principal,
            expires_at,
        })
    }
}
