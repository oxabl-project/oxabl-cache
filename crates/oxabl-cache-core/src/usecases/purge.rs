//! Deleting entries of old epochs or of one personal namespace.

use crate::domain::namespace::Namespace;
use crate::domain::principal::Caller;
use crate::domain::scope::Scope;
use crate::engine::Engine;
use crate::error::Result;
use crate::ids::{CacheId, PrincipalId};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::entry_removal::remove_entries;
use crate::usecases::scoped_cache::scoped_cache;

/// What to purge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurgeScope {
    /// Every entry of an epoch below this one.
    BelowEpoch(u32),
    /// Every entry of one principal's personal namespace.
    PersonalOf(PrincipalId),
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Deletes the matching entries and returns how many. Needs `cache:admin`.
    /// Bytes follow through the usual grace and sweep.
    pub async fn purge(&self, caller: &Caller, cache: CacheId, scope: PurgeScope) -> Result<u64> {
        let authorized = caller.clone();
        self.meta
            .transact(move |tx| {
                scoped_cache(tx, &authorized, &cache, Scope::CacheAdmin)?;
                Ok(())
            })
            .await?;
        let batch = self.config.gc_batch;
        let mut total = 0;
        loop {
            let now = self.now();
            let removed = self
                .meta
                .transact(move |tx| {
                    let entries = match scope {
                        PurgeScope::BelowEpoch(epoch) => {
                            tx.entries_below_epoch(&cache, epoch, batch)?
                        }
                        PurgeScope::PersonalOf(owner) => tx.entries_of_namespace(
                            &cache,
                            &Namespace::Personal(owner).key(),
                            batch,
                        )?,
                    };
                    Ok(remove_entries(tx, &cache, &entries, now, |_| false)?.entries)
                })
                .await?;
            if removed == 0 {
                return Ok(total);
            }
            total += removed;
        }
    }
}
