//! Deleting caches and tenants: a tombstone at once, removal in batches by the GC leader.

use crate::domain::principal::Caller;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::{CacheId, TenantId};
use crate::ports::{Clock, MetadataStore, ObjectKey, ObjectStore, PackAssembler, TxScope};

/// What one teardown pass removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TeardownReport {
    /// Objects deleted from the store.
    pub objects: u64,
    /// Caches whose rows are gone.
    pub caches: u64,
    /// Tenants whose rows are gone.
    pub tenants: u64,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Deletes a cache of the caller's tenant. Needs `tenant:admin`. Every request
    /// for the cache is `NotFound` from now on; the garbage collector removes its
    /// bytes and rows after the grace. A cache already deleted is `NotFound`.
    pub async fn delete_cache(&self, caller: &Caller, cache: CacheId) -> Result<()> {
        caller.require_tenant_admin()?;
        let tenant = caller.tenant;
        let now = self.now();
        self.meta
            .transact_in(TxScope::TenantCache(tenant, cache), move |tx| {
                if tx.tombstone_cache(&tenant, &cache, now)? {
                    Ok(())
                } else {
                    Err(Error::NotFound)
                }
            })
            .await
    }

    /// Deletes a tenant and every cache of it (an operator action). Its credentials
    /// stop working and its caches are `NotFound` at once; the garbage collector
    /// removes bytes and rows after the grace. A tenant already deleted is `NotFound`.
    pub async fn delete_tenant(&self, tenant: TenantId) -> Result<()> {
        let now = self.now();
        self.meta
            .transact(move |tx| {
                if tx.tombstone_tenant(&tenant, now)? {
                    Ok(())
                } else {
                    Err(Error::NotFound)
                }
            })
            .await
    }

    /// Advances the removal of deleted caches and tenants whose grace has passed:
    /// per cache, one batch of objects, then one batch of rows per pass; a tenant's
    /// rows go once its caches are gone. Every pass resumes where the last stopped.
    pub async fn gc_teardown(&self) -> Result<TeardownReport> {
        let (cutoff, batch) = (self.now() - self.config.grace_secs, self.config.gc_batch);
        let mut report = TeardownReport::default();
        let caches = self
            .meta
            .read(move |tx| tx.tombstoned_caches(cutoff, batch))
            .await?;
        for (cache, tenant) in caches {
            let objects = self.store.list(&cache).await?;
            if !objects.is_empty() {
                self.require_gc_leadership().await?;
                for object in objects.iter().take(batch) {
                    let key = ObjectKey {
                        cache,
                        pack: object.pack,
                    };
                    self.store.delete(&key).await?;
                    report.objects += 1;
                }
                continue;
            }
            let removed = self
                .gc_transact_in(TxScope::TenantCache(tenant, cache), move |tx| {
                    tx.drop_cache_rows(&cache, batch)
                })
                .await?;
            report.caches += u64::from(removed);
        }
        let tenants = self
            .meta
            .read(move |tx| tx.tombstoned_tenants_without_caches(cutoff, batch))
            .await?;
        for tenant in tenants {
            let removed = self
                .gc_transact_in(TxScope::Tenant(tenant), move |tx| {
                    tx.drop_tenant_rows(&tenant, batch)
                })
                .await?;
            report.tenants += u64::from(removed);
        }
        Ok(report)
    }
}
