//! Looking up entries by keyed id, in plan (presence) or fetch (records) mode.

use crate::domain::entry::EntryHit;
use crate::domain::namespace::{Namespace, NamespaceKey, NamespaceKind};
use crate::domain::principal::Caller;
use crate::domain::scope::Scope;
use crate::domain::usage::Metric;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ids::{CacheId, LookupId, PackId, SegmentId};
use crate::limits::LOOKUP_IDS;
use crate::ports::{Clock, MetadataStore, ObjectKey, ObjectStore, PackAssembler, SignedUrl};
use crate::time::{day_of, month_bounds_of_day};
use crate::usecases::scoped_cache::scoped_cache;
use std::collections::HashMap;

/// Which namespaces a lookup reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamespaceSet {
    /// Read the trusted namespace.
    pub trusted: bool,
    /// Read the caller's own personal namespace.
    pub personal: bool,
}

/// A lookup request.
#[derive(Debug, Clone)]
pub struct LookupRequest {
    /// The cache to read.
    pub cache: CacheId,
    /// The namespaces to read.
    pub namespaces: NamespaceSet,
    /// The keyed lookup ids, at most 10,000.
    pub ids: Vec<LookupId>,
    /// Fetch mode: return records and segment locations.
    pub records: bool,
    /// Fetch mode only: sign one GET URL per pack involved.
    pub urls: bool,
}

/// A pack involved in a fetch-mode answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackLocator {
    /// The pack.
    pub pack: PackId,
    /// A GET URL when requested.
    pub url: Option<SignedUrl>,
}

/// Where a segment lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentLocation {
    /// Index into the response's pack list.
    pub pack_index: usize,
    /// Absolute offset in the pack.
    pub offset: u64,
    /// Length in bytes.
    pub length: u32,
}

/// One hit: the request position, the namespace it was found in, and in fetch
/// mode its location and sealed record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupHit {
    /// Position of the id in the request.
    pub index: usize,
    /// The namespace the entry lives in.
    pub namespace: NamespaceKind,
    /// Segment location, in fetch mode.
    pub location: Option<SegmentLocation>,
    /// The sealed record, in fetch mode.
    pub record: Option<Vec<u8>>,
}

/// A dictionary a returned entry needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryLocation {
    /// The dictionary segment id.
    pub id: SegmentId,
    /// Where it lives.
    pub location: SegmentLocation,
}

/// The answer to a lookup.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LookupResponse {
    /// Packs involved, in first-use order; empty in plan mode.
    pub packs: Vec<PackLocator>,
    /// Hits in request order, trusted before personal for one id.
    pub hits: Vec<LookupHit>,
    /// Dictionaries the returned entries need, once each; empty in plan mode.
    pub dictionaries: Vec<DictionaryLocation>,
}

struct RawLookup {
    tenant: crate::ids::TenantId,
    hits: Vec<(usize, EntryHit)>,
    dictionaries: Vec<(SegmentId, PackId, u64, u32)>,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Looks up ids in the trusted namespace and the caller's own personal
    /// namespace. Needs `read`. Fetch mode is refused with `QuotaExceeded` once
    /// the tenant's monthly egress is used up; plan mode keeps working.
    pub async fn lookup(&self, caller: &Caller, request: LookupRequest) -> Result<LookupResponse> {
        if request.ids.len() > LOOKUP_IDS {
            return Err(Error::TooLarge);
        }
        let now = self.now();
        let today = day_of(now);
        let personal = Namespace::Personal(caller.principal).key();
        let mut namespaces: Vec<NamespaceKey> = Vec::new();
        if request.namespaces.trusted {
            namespaces.push(Namespace::Trusted.key());
        }
        if request.namespaces.personal {
            namespaces.push(personal);
        }
        let cache_id = request.cache;
        let records = request.records;
        let id_count = request.ids.len();
        let ids = request.ids;
        let caller_copy = caller.clone();
        let raw = self
            .meta
            .read(move |tx| {
                let cache = scoped_cache(tx, &caller_copy, &cache_id, Scope::Read)?;
                if records {
                    let tenant = tx.tenant(&cache.tenant)?.ok_or(Error::NotFound)?;
                    if let Some(limit) = tenant.egress_bytes {
                        let (from, to) = month_bounds_of_day(today);
                        let used = tx.tenant_metric(&tenant.id, Metric::EgressBytes, from, to)?;
                        if used >= limit {
                            return Err(Error::QuotaExceeded);
                        }
                    }
                }
                let mut positions: HashMap<LookupId, Vec<usize>> = HashMap::new();
                for (index, id) in ids.iter().enumerate() {
                    positions.entry(*id).or_default().push(index);
                }
                let found = tx.lookup_entries(&cache_id, &namespaces, &ids, records)?;
                let mut hits = Vec::with_capacity(found.len());
                for hit in found {
                    for index in positions.get(&hit.lookup_id).into_iter().flatten() {
                        hits.push((*index, hit.clone()));
                    }
                }
                hits.sort_by_key(|(index, hit)| {
                    (*index, hit.namespace != Namespace::Trusted.key())
                });
                let mut dictionaries = Vec::new();
                if records {
                    let mut wanted: Vec<SegmentId> =
                        hits.iter().filter_map(|(_, hit)| hit.dict_id).collect();
                    wanted.sort();
                    wanted.dedup();
                    let live = tx.live_dictionaries(&cache_id, &wanted)?;
                    for id in wanted {
                        if let Some(segment) = live.get(&id) {
                            dictionaries.push((id, segment.pack, segment.offset, segment.length));
                        }
                    }
                }
                Ok(RawLookup {
                    tenant: cache.tenant,
                    hits,
                    dictionaries,
                })
            })
            .await?;

        let mut response = LookupResponse::default();
        let mut pack_index: HashMap<PackId, usize> = HashMap::new();
        let mut locate = |pack: PackId, offset: u64, length: u32| {
            let index = *pack_index.entry(pack).or_insert_with(|| {
                response.packs.push(PackLocator { pack, url: None });
                response.packs.len() - 1
            });
            SegmentLocation {
                pack_index: index,
                offset,
                length,
            }
        };
        let mut egress = 0u64;
        let mut hits = Vec::with_capacity(raw.hits.len());
        for (index, hit) in raw.hits {
            if hit.access_day < today {
                self.access
                    .record(cache_id, hit.namespace, hit.lookup_id, today);
            }
            let location = records.then(|| {
                egress += u64::from(hit.length);
                locate(hit.pack, hit.offset, hit.length)
            });
            hits.push(LookupHit {
                index,
                namespace: Namespace::from_key(hit.namespace).kind(),
                location,
                record: hit.record,
            });
        }
        let mut dictionaries = Vec::with_capacity(raw.dictionaries.len());
        for (id, pack, offset, length) in raw.dictionaries {
            egress += u64::from(length);
            dictionaries.push(DictionaryLocation {
                id,
                location: locate(pack, offset, length),
            });
        }
        response.hits = hits;
        response.dictionaries = dictionaries;

        if request.urls && records {
            let expires_at = now + self.config.url_lifetime_secs;
            for locator in &mut response.packs {
                let key = ObjectKey {
                    cache: cache_id,
                    pack: locator.pack,
                };
                locator.url = Some(self.store.get_url(&key, expires_at).await?);
            }
        }

        let tenant = raw.tenant;
        let counted = u64::try_from(response.hits.len()).unwrap_or(u64::MAX);
        self.count(tenant, cache_id, Metric::Requests, 1);
        self.count(
            tenant,
            cache_id,
            Metric::LookupIds,
            u64::try_from(id_count).unwrap_or(u64::MAX),
        );
        self.count(tenant, cache_id, Metric::Hits, counted);
        self.count(tenant, cache_id, Metric::EgressBytes, egress);
        Ok(response)
    }
}
