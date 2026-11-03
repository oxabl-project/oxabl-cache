//! Reading the current roster.

use crate::domain::principal::Caller;
use crate::domain::roster::StoredRoster;
use crate::domain::scope::Scope;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::CacheId;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::scoped_cache::scoped_cache;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// The current roster bytes and version. Needs `read`; `NotFound` before the first roster.
    pub async fn get_roster(&self, caller: &Caller, cache: CacheId) -> Result<StoredRoster> {
        let caller = caller.clone();
        self.meta
            .read(move |tx| {
                scoped_cache(tx, &caller, &cache, Scope::Read)?;
                tx.roster(&cache)?.ok_or(Error::NotFound)
            })
            .await
    }
}
