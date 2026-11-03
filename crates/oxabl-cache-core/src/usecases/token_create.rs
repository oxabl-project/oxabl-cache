//! Creating API tokens.

use crate::domain::credential::{Credential, CredentialKind};
use crate::domain::principal::Caller;
use crate::domain::scope::{Grant, Scope, ScopeSet};
use crate::engine::Engine;
use crate::error::{Conflict, Error, Result};
use crate::ids::{CacheId, CredentialId, PrincipalId};
use crate::ports::TxScope;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::time::DAY_SECS;
use crate::usecases::credential_text::{generate, hash_secret};

const DEFAULT_TOKEN_DAYS: i64 = 90;
const MAX_TOKEN_DAYS: i64 = 365;

/// What to create.
#[derive(Debug, Clone)]
pub struct TokenSpec {
    /// Operator-visible label.
    pub name: String,
    /// The principal the token acts as; a fresh principal when `None`.
    pub principal: Option<PrincipalId>,
    /// The scopes the token carries; `tenant:admin` applies tenant-wide.
    pub scopes: ScopeSet,
    /// The caches the other scopes apply to.
    pub caches: Vec<CacheId>,
    /// Lifetime in seconds; 90 days when `None`, at most a year.
    pub expires_in_secs: Option<i64>,
}

/// A freshly created credential. The text is shown once and never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCredential {
    /// The credential text to hand to the client.
    pub secret: String,
    /// The public id.
    pub id: CredentialId,
    /// The principal it acts as.
    pub principal: PrincipalId,
    /// Expiry, unix seconds.
    pub expires_at: i64,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Creates an API token. Needs `tenant:admin`.
    pub async fn create_token(&self, caller: &Caller, spec: TokenSpec) -> Result<NewCredential> {
        caller.require_tenant_admin()?;
        let now = self.now();
        let lifetime = spec
            .expires_in_secs
            .unwrap_or(DEFAULT_TOKEN_DAYS * DAY_SECS);
        if lifetime <= 0 || lifetime > MAX_TOKEN_DAYS * DAY_SECS {
            return Err(Error::bad_request("token lifetime out of range"));
        }
        if spec.scopes.is_empty() || spec.name.is_empty() {
            return Err(Error::bad_request("a token needs a name and scopes"));
        }
        let (id, secret, text) = generate(CredentialKind::Token);
        let tenant = caller.tenant;
        let principal = spec.principal.unwrap_or_else(PrincipalId::random);
        let existing_principal = spec.principal;
        let data_scopes: ScopeSet = [
            Scope::Read,
            Scope::WritePersonal,
            Scope::WriteTrusted,
            Scope::CacheAdmin,
        ]
        .into_iter()
        .filter(|scope| spec.scopes.contains(*scope))
        .collect();
        let tenant_admin = spec.scopes.contains(Scope::TenantAdmin);
        if !data_scopes.is_empty() && spec.caches.is_empty() {
            return Err(Error::bad_request("cache scopes need at least one cache"));
        }
        let expires_at = now + lifetime;
        self.meta
            .transact_in(TxScope::Tenant(tenant), move |tx| {
                if let Some(existing) = existing_principal {
                    match tx.user(&existing)? {
                        Some(user) if user.tenant == tenant && user.disabled_at.is_none() => {}
                        _ => return Err(Error::NotFound),
                    }
                }
                let mut grants = Vec::new();
                for cache in &spec.caches {
                    tx.cache(&tenant, cache)?.ok_or(Error::NotFound)?;
                    if !data_scopes.is_empty() {
                        grants.push(Grant {
                            cache: Some(*cache),
                            scopes: data_scopes,
                        });
                    }
                }
                if tenant_admin {
                    grants.push(Grant {
                        cache: None,
                        scopes: ScopeSet::EMPTY.with(Scope::TenantAdmin),
                    });
                }
                let credential = Credential {
                    kind: CredentialKind::Token,
                    id,
                    tenant,
                    principal,
                    name: spec.name,
                    secret_hash: hash_secret(&secret),
                    created_at: now,
                    expires_at,
                    revoked_at: None,
                    grants,
                };
                if !tx.insert_credential(&credential)? {
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
