//! Reading the dictionary pointer.

use crate::domain::principal::Caller;
use crate::domain::roster::StoredDictionaryPointer;
use crate::domain::scope::Scope;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::CacheId;
use crate::ids::PackId;
use crate::ports::{Clock, MetadataStore, ObjectKey, ObjectStore, PackAssembler, SignedUrl};
use crate::usecases::scoped_cache::scoped_cache;

/// The dictionary pointer and where the segment it names can be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedDictionary {
    /// The stored pointer.
    pub pointer: StoredDictionaryPointer,
    /// The pack holding the dictionary segment.
    pub pack: PackId,
    /// Absolute offset of the segment in the pack.
    pub offset: u64,
    /// Length of the segment in bytes.
    pub length: u32,
    /// A GET URL for the pack.
    pub url: SignedUrl,
}

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
            .read(move |tx| {
                scoped_cache(tx, &caller, &cache, Scope::Read)?;
                tx.dictionary_pointer(&cache)?.ok_or(Error::NotFound)
            })
            .await
    }

    /// The current dictionary pointer with the location of its segment and a GET URL for
    /// the pack. Needs `read`; `NotFound` if no pointer was published or its segment is no
    /// longer live.
    pub async fn locate_dictionary(
        &self,
        caller: &Caller,
        cache: CacheId,
    ) -> Result<LocatedDictionary> {
        let caller = caller.clone();
        let (pointer, segment) = self
            .meta
            .transact(move |tx| {
                scoped_cache(tx, &caller, &cache, Scope::Read)?;
                let pointer = tx.dictionary_pointer(&cache)?.ok_or(Error::NotFound)?;
                let segment = tx
                    .live_dictionaries(&cache, &[pointer.segment_id])?
                    .remove(&pointer.segment_id)
                    .ok_or(Error::NotFound)?;
                Ok((pointer, segment))
            })
            .await?;
        let url = self
            .store
            .get_url(
                &ObjectKey {
                    cache,
                    pack: segment.pack,
                },
                self.now() + self.config.url_lifetime_secs,
            )
            .await?;
        Ok(LocatedDictionary {
            pointer,
            pack: segment.pack,
            offset: segment.offset,
            length: segment.length,
            url,
        })
    }
}
