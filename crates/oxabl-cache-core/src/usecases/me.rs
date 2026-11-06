//! What the caller can see: its tenant and the caches it holds grants on.

use crate::domain::cache::CacheQuota;
use crate::domain::principal::Caller;
use crate::domain::scope::{Scope, ScopeSet};
use crate::engine::Engine;
use crate::error::Result;
use crate::ids::{CacheId, PrincipalId, TenantId};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};

/// One cache the caller holds a grant on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleCache {
    pub id: CacheId,
    pub display_name: String,
    /// The caller's scopes on this cache; empty for a tenant administrator without any.
    pub scopes: ScopeSet,
    pub epoch: u32,
    pub roster_version: u64,
    pub quota: CacheQuota,
}

/// What `GET /v1/me` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me {
    pub tenant: TenantId,
    pub principal: PrincipalId,
    pub tenant_admin: bool,
    pub caches: Vec<VisibleCache>,
}

fn scopes_on(caller: &Caller, cache: &CacheId) -> ScopeSet {
    caller
        .grants
        .iter()
        .filter(|grant| grant.cache.as_ref() == Some(cache))
        .fold(ScopeSet::EMPTY, |set, grant| {
            [
                Scope::Read,
                Scope::WritePersonal,
                Scope::WriteTrusted,
                Scope::CacheAdmin,
            ]
            .into_iter()
            .filter(|scope| grant.scopes.contains(*scope))
            .fold(set, ScopeSet::with)
        })
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// The caller's tenant and every cache of it the caller holds a scope on; a tenant
    /// administrator sees every cache of the tenant. Needs any credential.
    pub async fn me(&self, caller: &Caller) -> Result<Me> {
        let caller = caller.clone();
        self.meta
            .read(move |tx| {
                let admin = caller.is_tenant_admin();
                let mut caches = Vec::new();
                for cache in tx.caches_of(&caller.tenant)? {
                    let scopes = scopes_on(&caller, &cache.id);
                    if scopes.is_empty() && !admin {
                        continue;
                    }
                    let totals = tx.pack_totals(&cache.id)?;
                    caches.push(VisibleCache {
                        id: cache.id,
                        display_name: cache.display_name,
                        scopes,
                        epoch: cache.epoch,
                        roster_version: cache.roster_version,
                        quota: CacheQuota {
                            hard_bytes: cache.hard_bytes,
                            live_bytes: cache.live_bytes,
                            pending_bytes: cache.pending_bytes,
                            stored_bytes: totals.stored_bytes,
                            reclaimable_bytes: totals.reclaimable_bytes,
                        },
                    });
                }
                caches.sort_by(|a, b| a.display_name.cmp(&b.display_name));
                Ok(Me {
                    tenant: caller.tenant,
                    principal: caller.principal,
                    tenant_admin: admin,
                    caches,
                })
            })
            .await
    }
}
