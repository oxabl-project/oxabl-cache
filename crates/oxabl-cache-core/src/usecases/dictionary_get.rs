//! Reading the dictionary pointer.

use crate::domain::principal::Caller;
use crate::domain::roster::StoredDictionaryPointer;
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
    /// The current dictionary pointer. Needs `read`; `NotFound` if none was published.
    pub async fn get_dictionary(
        &self,
        caller: &Caller,
        cache: CacheId,
    ) -> Result<StoredDictionaryPointer> {
        let caller = caller.clone();
        self.meta
            .transact(move |tx| {
                scoped_cache(tx, &caller, &cache, Scope::Read)?;
                tx.dictionary_pointer(&cache)?.ok_or(Error::NotFound)
            })
            .await
    }
}
