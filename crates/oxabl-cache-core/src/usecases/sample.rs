//! Sampling the live lookup ids of the trusted namespace.

use crate::domain::namespace::Namespace;
use crate::domain::principal::Caller;
use crate::domain::scope::Scope;
use crate::domain::usage::Metric;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::{CacheId, LookupId};
use crate::limits::SAMPLE_IDS;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::scoped_cache::scoped_cache;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Up to `count` live lookup ids of the trusted namespace: the ids that follow a
    /// random point of the id space, wrapping around, so a cache with fewer than
    /// `count` live entries returns them all. Ids are keyed hashes, so neighbours in
    /// the id space are unrelated entries. Needs `read`; `count` is 1 to 1,000.
    pub async fn sample(
        &self,
        caller: &Caller,
        cache: CacheId,
        count: usize,
    ) -> Result<Vec<LookupId>> {
        if count == 0 || count > SAMPLE_IDS {
            return Err(Error::bad_request(format!(
                "n must be between 1 and {SAMPLE_IDS}"
            )));
        }
        let start = LookupId::random();
        let namespace = Namespace::Trusted.key();
        let caller = caller.clone();
        let (tenant, ids) = self
            .meta
            .read(move |tx| {
                let found = scoped_cache(tx, &caller, &cache, Scope::Read)?;
                let mut ids = tx.live_lookup_ids_from(&cache, &namespace, &start, count)?;
                if ids.len() < count {
                    let wrapped = tx.live_lookup_ids_from(
                        &cache,
                        &namespace,
                        &LookupId::from_bytes([0; LookupId::LEN]),
                        count - ids.len(),
                    )?;
                    ids.extend(wrapped.into_iter().filter(|id| *id < start));
                }
                Ok((found.tenant, ids))
            })
            .await?;
        self.count(tenant, cache, Metric::Requests, 1);
        Ok(ids)
    }
}
