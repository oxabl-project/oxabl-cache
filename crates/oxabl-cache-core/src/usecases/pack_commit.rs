//! Committing a pack exactly once.

use crate::domain::entry::{EntrySpec, EntryStatus};
use crate::domain::namespace::{Namespace, NamespaceKind};
use crate::domain::pack::PackState;
use crate::domain::principal::Caller;
use crate::domain::scope::Scope;
use crate::domain::segment::SegmentSpec;
use crate::domain::usage::Metric;
use crate::engine::Engine;
use crate::error::{Conflict, Error, Result};
use crate::ids::{CacheId, PackId};
use crate::ports::{Clock, MetadataStore, ObjectKey, ObjectStore, PackAssembler};
use crate::usecases::commit_apply::{CommitInput, CommitOutput, answer_repeat, apply_commit};
use crate::usecases::commit_validate::validate_frame;
use crate::usecases::scoped_cache::scoped_cache;

/// A commit frame as parsed by the HTTP layer.
#[derive(Debug, Clone)]
pub struct CommitRequest {
    /// The cache the pack belongs to.
    pub cache: CacheId,
    /// The pack being committed.
    pub pack: PackId,
    /// The namespace kind the frame names.
    pub kind: NamespaceKind,
    /// BLAKE3 of the request body, recorded to answer repeats.
    pub digest: [u8; 32],
    /// The pack size the frame declares.
    pub pack_size: u64,
    /// The segment table.
    pub segments: Vec<SegmentSpec>,
    /// The entries, referencing the table by index.
    pub entries: Vec<EntrySpec>,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Commits a pack once and returns one status per entry. Authorization comes
    /// first and touches neither the pack's bytes nor the store. A repeat is
    /// answered from the record: same body, same statuses, no change.
    pub async fn commit_pack(
        &self,
        caller: &Caller,
        request: CommitRequest,
    ) -> Result<Vec<EntryStatus>> {
        let scope = match request.kind {
            NamespaceKind::Trusted => Scope::WriteTrusted,
            NamespaceKind::Personal => Scope::WritePersonal,
        };
        let namespace = match request.kind {
            NamespaceKind::Trusted => Namespace::Trusted,
            NamespaceKind::Personal => Namespace::Personal(caller.principal),
        };
        let key = ObjectKey {
            cache: request.cache,
            pack: request.pack,
        };
        let (cache_id, pack_id) = (request.cache, request.pack);
        let authorized = caller.clone();
        let (tenant, pack) = self
            .meta
            .transact(move |tx| {
                let cache = scoped_cache(tx, &authorized, &cache_id, scope)?;
                let pack = tx.pack(&cache_id, &pack_id)?.ok_or(Error::NotFound)?;
                if pack.uploader != authorized.principal {
                    return Err(Error::Forbidden("not the uploader of this pack"));
                }
                Ok((cache.tenant, pack))
            })
            .await?;

        if let (Some(digest), Some(statuses)) = (&pack.commit_digest, &pack.commit_statuses) {
            let current = self.store.head(&key).await?;
            if current.map(|head| head.etag) != pack.etag {
                self.queue_scrub(cache_id, pack_id).await?;
                return Err(Error::Conflict(Conflict::PackChanged));
            }
            return answer_repeat(statuses, digest, &request.digest);
        }
        if pack.state != PackState::Pending {
            return Err(Error::Conflict(Conflict::PackClosed));
        }
        if pack.namespace != namespace.key() {
            return Err(Error::bad_request("the pack belongs to another namespace"));
        }
        if request.pack_size != pack.size {
            return Err(Error::bad_request("pack size differs from the reservation"));
        }
        validate_frame(request.kind, pack.size, &request.segments, &request.entries)?;

        let head = match self.store.head(&key).await? {
            Some(head) if head.size == pack.size => head,
            _ => return Err(Error::Conflict(Conflict::PackIncomplete)),
        };

        let input = CommitInput {
            tenant,
            cache: cache_id,
            pack: pack_id,
            caller: caller.principal,
            namespace,
            digest: request.digest,
            etag: head.etag,
            segments: request.segments,
            entries: request.entries,
            now: self.now(),
        };
        let CommitOutput {
            statuses,
            recorded,
            pack_size,
        } = self
            .meta
            .transact(move |tx| apply_commit(tx, input))
            .await?;
        if recorded {
            let committed = statuses
                .iter()
                .filter(|status| **status == EntryStatus::Committed)
                .count();
            self.count(tenant, cache_id, Metric::IngressBytes, pack_size);
            self.count(
                tenant,
                cache_id,
                Metric::EntriesCommitted,
                u64::try_from(committed).unwrap_or(u64::MAX),
            );
            self.count(tenant, cache_id, Metric::Requests, 1);
        }
        Ok(statuses)
    }

    async fn queue_scrub(&self, cache: CacheId, pack: PackId) -> Result<()> {
        let now = self.now();
        self.meta
            .transact(move |tx| tx.queue_scrub(&cache, &pack, now))
            .await
    }
}
