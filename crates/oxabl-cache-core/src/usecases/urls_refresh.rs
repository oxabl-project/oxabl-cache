//! Issuing fresh GET URLs for packs a caller can read.

use crate::domain::namespace::Namespace;
use crate::domain::pack::PackState;
use crate::domain::principal::Caller;
use crate::domain::scope::Scope;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::{CacheId, PackId};
use crate::limits::LOOKUP_IDS;
use crate::ports::{Clock, MetadataStore, ObjectKey, ObjectStore, PackAssembler, SignedUrl};
use crate::usecases::scoped_cache::scoped_cache;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// A GET URL per live pack the caller may read (trusted or the caller's own
    /// personal namespace). Other packs are omitted. Needs `read`.
    pub async fn refresh_urls(
        &self,
        caller: &Caller,
        cache: CacheId,
        packs: Vec<PackId>,
    ) -> Result<Vec<(PackId, SignedUrl)>> {
        if packs.len() > LOOKUP_IDS {
            return Err(Error::TooLarge);
        }
        let caller = caller.clone();
        let principal = caller.principal;
        let readable = self
            .meta
            .read(move |tx| {
                scoped_cache(tx, &caller, &cache, Scope::Read)?;
                let mut readable = Vec::new();
                for id in packs {
                    let Some(pack) = tx.pack(&cache, &id)? else {
                        continue;
                    };
                    let visible = match Namespace::from_key(pack.namespace) {
                        Namespace::Trusted => true,
                        Namespace::Personal(owner) => owner == principal,
                    };
                    if pack.state == PackState::Live && visible {
                        readable.push(id);
                    }
                }
                Ok(readable)
            })
            .await?;
        let expires_at = self.url_expiry(self.now());
        let mut urls = Vec::with_capacity(readable.len());
        for pack in readable {
            let url = self
                .store
                .get_url(&ObjectKey { cache, pack }, expires_at)
                .await?;
            urls.push((pack, url));
        }
        Ok(urls)
    }
}
