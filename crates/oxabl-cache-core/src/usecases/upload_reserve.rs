//! Reserving packs: quota admission, a pending row with a lease, a PUT URL.

use crate::domain::namespace::{Namespace, NamespaceKind};
use crate::domain::pack::{Pack, PackState};
use crate::domain::principal::Caller;
use crate::domain::scope::Scope;
use crate::engine::Engine;
use crate::error::{Conflict, Error, Result};
use crate::ids::{CacheId, ClientRef, PackId};
use crate::limits::{PACK_BYTES, PACK_MIN_BYTES};
use crate::ports::TxScope;
use crate::ports::{Clock, MetadataStore, ObjectKey, ObjectStore, PackAssembler, SignedUrl};
use crate::usecases::admission::admit;
use crate::usecases::scoped_cache::scoped_cache;

/// One pack the client wants to upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackRequest {
    /// Retry key, 16 random bytes per pack.
    pub client_ref: ClientRef,
    /// Exact size of the pack object.
    pub size: u64,
    /// SHA-256 of the pack, required only when the store verifies checksums.
    pub sha256: Option<[u8; 32]>,
}

/// The answer for one requested pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadOutcome {
    /// Reserved: PUT the bytes to the URL, then commit.
    Reserved {
        /// The pack id.
        pack: PackId,
        /// The PUT capability.
        url: SignedUrl,
    },
    /// The pack was committed before; no URL is issued.
    Committed {
        /// The pack id.
        pack: PackId,
    },
    /// A quota refuses the pack. The client treats this as "not stored".
    QuotaExceeded,
}

enum Reservation {
    Fresh(PackId),
    Renewed(PackId),
    Committed(PackId),
    Refused,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Reserves packs in the trusted or the caller's personal namespace, one
    /// outcome per pack in request order. Needs `write:trusted` or
    /// `write:personal` to match. A retry of a pending reservation returns the
    /// same pack with a fresh URL and reserves nothing more.
    pub async fn reserve_uploads(
        &self,
        caller: &Caller,
        cache: CacheId,
        kind: NamespaceKind,
        packs: Vec<PackRequest>,
    ) -> Result<Vec<UploadOutcome>> {
        let scope = match kind {
            NamespaceKind::Trusted => Scope::WriteTrusted,
            NamespaceKind::Personal => Scope::WritePersonal,
        };
        for pack in &packs {
            if pack.size > PACK_BYTES {
                return Err(Error::TooLarge);
            }
            if pack.size < PACK_MIN_BYTES {
                return Err(Error::bad_request(
                    "pack smaller than a pack header and trailer",
                ));
            }
        }
        let namespace = match kind {
            NamespaceKind::Trusted => Namespace::Trusted,
            NamespaceKind::Personal => Namespace::Personal(caller.principal),
        };
        let mut outcomes = Vec::with_capacity(packs.len());
        for request in packs {
            let reservation = self
                .reserve_one(caller, cache, kind, namespace, scope, request)
                .await?;
            outcomes.push(match reservation {
                Reservation::Refused => UploadOutcome::QuotaExceeded,
                Reservation::Committed(pack) => UploadOutcome::Committed { pack },
                Reservation::Fresh(pack) | Reservation::Renewed(pack) => {
                    let expires_at = self.now() + self.config.url_lifetime_secs;
                    let url = self
                        .store
                        .put_url(
                            &ObjectKey { cache, pack },
                            request.size,
                            request.sha256,
                            expires_at,
                        )
                        .await?;
                    UploadOutcome::Reserved { pack, url }
                }
            });
        }
        Ok(outcomes)
    }

    async fn reserve_one(
        &self,
        caller: &Caller,
        cache: CacheId,
        kind: NamespaceKind,
        namespace: Namespace,
        scope: Scope,
        request: PackRequest,
    ) -> Result<Reservation> {
        let tenant = caller.tenant;
        let caller = caller.clone();
        let config = self.config.clone();
        let now = self.now();
        self.meta
            .transact_in(TxScope::TenantCache(tenant, cache), move |tx| {
                let found = scoped_cache(tx, &caller, &cache, scope)?;
                if let Some(existing) =
                    tx.pack_by_client_ref(&cache, &caller.principal, &request.client_ref)?
                {
                    if existing.namespace != namespace.key() || existing.size != request.size {
                        return Err(Error::bad_request(
                            "client_ref was used for a different pack",
                        ));
                    }
                    return match existing.state {
                        PackState::Live => Ok(Reservation::Committed(existing.id)),
                        PackState::Pending
                            if existing.commit_digest.is_none() && existing.lease_until >= now =>
                        {
                            Ok(Reservation::Renewed(existing.id))
                        }
                        _ => Err(Error::Conflict(Conflict::PackClosed)),
                    };
                }
                if !admit(tx, &config, &found, kind, namespace, request.size, now)? {
                    return Ok(Reservation::Refused);
                }
                let pack = Pack {
                    cache,
                    id: PackId::random(),
                    namespace: namespace.key(),
                    uploader: caller.principal,
                    client_ref: request.client_ref,
                    size: request.size,
                    etag: None,
                    live_bytes: 0,
                    state: PackState::Pending,
                    lease_until: now + config.lease_secs,
                    created_at: now,
                    dead_since: None,
                    commit_digest: None,
                    commit_statuses: None,
                };
                tx.insert_pack(&pack)?;
                let pending = i64::try_from(request.size)
                    .map_err(|_| Error::bad_request("pack size out of range"))?;
                tx.adjust_cache_bytes(&cache, 0, pending)?;
                Ok(Reservation::Fresh(pack.id))
            })
            .await
    }
}
