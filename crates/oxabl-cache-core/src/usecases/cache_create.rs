//! Creating caches and registering their owner keys.

use crate::domain::cache::Cache;
use crate::domain::principal::Caller;
use crate::domain::roster::PublicKey;
use crate::engine::Engine;
use crate::error::{Conflict, Error, Result};
use crate::ids::CacheId;
use crate::ports::TxScope;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};

/// What a new cache is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheSpec {
    /// Customer-chosen name.
    pub display_name: String,
    /// Retention in days; the configured default when `None`.
    pub retention_days: Option<u32>,
    /// Hard quota in bytes; the configured default when `None`.
    pub hard_bytes: Option<u64>,
    /// At least one owner key; the server checks roster signatures against these.
    pub owner_keys: Vec<PublicKey>,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Creates a cache in the caller's tenant. Needs `tenant:admin`. A display name
    /// is unique within the tenant: a second cache of that name is `Conflict(Exists)`.
    pub async fn create_cache(&self, caller: &Caller, spec: CacheSpec) -> Result<CacheId> {
        caller.require_tenant_admin()?;
        if spec.owner_keys.is_empty() || spec.display_name.is_empty() {
            return Err(Error::bad_request("a cache needs a name and an owner key"));
        }
        let cache = Cache {
            id: CacheId::random(),
            tenant: caller.tenant,
            display_name: spec.display_name,
            retention_days: spec
                .retention_days
                .unwrap_or(self.config.default_retention_days),
            hard_bytes: spec.hard_bytes.unwrap_or(self.config.default_hard_bytes),
            epoch: 0,
            roster_version: 0,
            live_bytes: 0,
            pending_bytes: 0,
            created_at: self.now(),
        };
        let id = cache.id;
        let keys = spec.owner_keys;
        self.meta
            .transact_in(TxScope::Tenant(cache.tenant), move |tx| {
                if tx
                    .caches_of(&cache.tenant)?
                    .iter()
                    .any(|existing| existing.display_name == cache.display_name)
                {
                    return Err(Error::Conflict(Conflict::Exists));
                }
                if !tx.insert_cache(&cache)? {
                    return Err(Error::Conflict(Conflict::Exists));
                }
                tx.set_owner_keys(&cache.id, &keys)
            })
            .await?;
        Ok(id)
    }

    /// Replaces the owner keys of a cache. Needs `tenant:admin`.
    pub async fn set_owner_keys(
        &self,
        caller: &Caller,
        cache: CacheId,
        keys: Vec<PublicKey>,
    ) -> Result<()> {
        caller.require_tenant_admin()?;
        if keys.is_empty() {
            return Err(Error::bad_request("a cache needs an owner key"));
        }
        let tenant = caller.tenant;
        self.meta
            .transact_in(TxScope::TenantCache(tenant, cache), move |tx| {
                tx.cache(&tenant, &cache)?.ok_or(Error::NotFound)?;
                tx.set_owner_keys(&cache, &keys)
            })
            .await
    }
}
