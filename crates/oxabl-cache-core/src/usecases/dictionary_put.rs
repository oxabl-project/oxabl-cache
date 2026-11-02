//! Moving the dictionary pointer.

use crate::domain::principal::Caller;
use crate::domain::roster::{DictionaryPointer, MemberRole, StoredDictionaryPointer};
use crate::domain::scope::Scope;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::CacheId;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::scoped_cache::scoped_cache;
use crate::usecases::segment_death::release_dictionary;
use crate::usecases::signature_check::verifies;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Moves the pointer to a live dictionary segment. Needs `write:trusted`.
    /// `expected_version` is the `If-Match` version (0 for none); the body must be
    /// `current + 1` of the cache's epoch, signed by a `ci` or `owner` key of the
    /// current roster. The previous dictionary is released to the mark rules.
    pub async fn put_dictionary(
        &self,
        caller: &Caller,
        cache: CacheId,
        expected_version: u64,
        pointer: DictionaryPointer,
    ) -> Result<()> {
        let caller = caller.clone();
        let now = self.now();
        self.meta
            .transact(move |tx| {
                let found = scoped_cache(tx, &caller, &cache, Scope::WriteTrusted)?;
                let previous = tx.dictionary_pointer(&cache)?;
                let current = previous.as_ref().map_or(0, |p| p.version);
                if current != expected_version {
                    return Err(Error::PreconditionFailed);
                }
                if pointer.version != current + 1 {
                    return Err(Error::bad_request("pointer version must be current + 1"));
                }
                if pointer.epoch != found.epoch {
                    return Err(Error::bad_request("pointer epoch is not the cache epoch"));
                }
                let signer = &pointer.signature;
                let trusted_role = tx
                    .signer_roles(&cache, &signer.key)?
                    .into_iter()
                    .any(|role| matches!(role, MemberRole::Ci | MemberRole::Owner));
                if !trusted_role
                    || !verifies(&signer.key, &pointer.signed_message, &signer.signature)
                {
                    return Err(Error::bad_request(
                        "pointer is not signed by a ci or owner key",
                    ));
                }
                if !tx
                    .live_dictionaries(&cache, &[pointer.segment_id])?
                    .contains_key(&pointer.segment_id)
                {
                    return Err(Error::bad_request("the segment is not a live dictionary"));
                }
                let stored = StoredDictionaryPointer {
                    version: pointer.version,
                    epoch: pointer.epoch,
                    segment_id: pointer.segment_id,
                    bytes: pointer.bytes,
                };
                if !tx.replace_dictionary_pointer(&cache, expected_version, &stored, now)? {
                    return Err(Error::PreconditionFailed);
                }
                if let Some(old) = previous.filter(|p| p.segment_id != stored.segment_id) {
                    release_dictionary(tx, &cache, &old.segment_id, now)?;
                }
                Ok(())
            })
            .await
    }
}
