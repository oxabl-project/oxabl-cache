//! What a tenant administrator can list.

use crate::domain::cache::Cache;
use crate::domain::credential::TokenSummary;
use crate::domain::principal::Caller;
use crate::domain::roster::PublicKey;
use crate::domain::scope::Grant;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::{CacheId, PrincipalId};
use crate::ports::metadata::User;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// The tenant's caches. Needs `tenant:admin`.
    pub async fn list_caches(&self, caller: &Caller) -> Result<Vec<Cache>> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        self.meta.read(move |tx| tx.caches_of(&tenant)).await
    }

    /// The owner keys of a cache. Needs `tenant:admin`.
    pub async fn get_owner_keys(&self, caller: &Caller, cache: CacheId) -> Result<Vec<PublicKey>> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        self.meta
            .transact(move |tx| {
                tx.cache(&tenant, &cache)?.ok_or(Error::NotFound)?;
                tx.owner_keys(&cache)
            })
            .await
    }

    /// The tenant's API tokens, without secrets. Needs `tenant:admin`.
    pub async fn list_tokens(&self, caller: &Caller) -> Result<Vec<TokenSummary>> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        self.meta.read(move |tx| tx.tokens_of(&tenant)).await
    }

    /// The tenant's users. Needs `tenant:admin`.
    pub async fn list_users(&self, caller: &Caller) -> Result<Vec<User>> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        self.meta.read(move |tx| tx.users_of(&tenant)).await
    }

    /// Every standing grant of the tenant. Needs `tenant:admin`.
    pub async fn list_grants(&self, caller: &Caller) -> Result<Vec<(PrincipalId, Grant)>> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        self.meta.read(move |tx| tx.tenant_grants(&tenant)).await
    }
}
