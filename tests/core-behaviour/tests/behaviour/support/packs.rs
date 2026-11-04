//! Building, uploading and committing packs the way a client does.

use super::env::Env;
use oxabl_cache_core::domain::entry::{EntrySpec, EntryStatus};
use oxabl_cache_core::domain::namespace::NamespaceKind;
use oxabl_cache_core::domain::principal::Caller;
use oxabl_cache_core::domain::segment::{SegmentKind, SegmentSpec};
use oxabl_cache_core::ids::{LookupId, PackId, SegmentId};
use oxabl_cache_core::ports::{ObjectKey, PackAssembler, PackPart, SignedUrl};
use oxabl_cache_core::usecases::{
    CommitRequest, LookupRequest, NamespaceSet, PackRequest, UploadOutcome,
};
use oxabl_cache_core::{Error, Result};
use oxabl_cache_store_fs::UrlRequest;

use super::assembler::TestAssembler;

/// A pack built locally: its bytes and the segment table a commit would carry.
#[derive(Clone)]
pub struct Built {
    pub bytes: Vec<u8>,
    pub segments: Vec<SegmentSpec>,
}

/// Distinct sealed-looking bytes of a given length; `seed` makes the segment id unique.
pub fn sealed(seed: u32, length: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(length);
    let mut state = u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    while bytes.len() < length {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        bytes.extend_from_slice(&state.to_le_bytes());
    }
    bytes.truncate(length);
    bytes
}

pub fn lookup_id(seed: u32) -> LookupId {
    LookupId::from_bytes(*blake3::hash(&seed.to_be_bytes()).as_bytes())
}

pub fn entry(lookup_seed: u32, segment_index: u32) -> EntrySpec {
    EntrySpec {
        lookup_id: lookup_id(lookup_seed),
        segment_index,
        dict_id: None,
        epoch: 0,
        record: sealed(lookup_seed.wrapping_add(900_000), 256),
        header_ok: true,
    }
}

impl Env {
    pub fn cache_assemble(&self, parts: &[(Vec<u8>, SegmentKind)]) -> Built {
        let assembled = TestAssembler
            .assemble(
                &self.cache,
                parts
                    .iter()
                    .map(|(bytes, _)| PackPart {
                        id: SegmentId::of_bytes(bytes),
                        bytes: bytes.clone(),
                    })
                    .collect(),
            )
            .expect("assemble");
        let segments = assembled
            .placements
            .iter()
            .zip(parts)
            .map(|(placement, (_, kind))| SegmentSpec {
                id: placement.id,
                offset: placement.offset,
                length: placement.length,
                kind: *kind,
            })
            .collect();
        Built {
            bytes: assembled.bytes,
            segments,
        }
    }

    pub fn artifacts(&self, seeds: &[u32], length: usize) -> Built {
        let parts: Vec<_> = seeds
            .iter()
            .map(|seed| (sealed(*seed, length), SegmentKind::Artifact))
            .collect();
        self.cache_assemble(&parts)
    }

    pub async fn reserve(
        &self,
        caller: &Caller,
        kind: NamespaceKind,
        built: &Built,
    ) -> Result<UploadOutcome> {
        let mut client_ref = [0u8; 16];
        getrandom_fill(&mut client_ref);
        self.reserve_with_ref(
            caller,
            kind,
            built,
            oxabl_cache_core::ids::ClientRef::from_bytes(client_ref),
        )
        .await
    }

    pub async fn reserve_with_ref(
        &self,
        caller: &Caller,
        kind: NamespaceKind,
        built: &Built,
        client_ref: oxabl_cache_core::ids::ClientRef,
    ) -> Result<UploadOutcome> {
        let outcomes = self
            .engine
            .reserve_uploads(
                caller,
                self.cache,
                kind,
                vec![PackRequest {
                    client_ref,
                    size: built.bytes.len() as u64,
                    sha256: None,
                }],
            )
            .await?;
        Ok(outcomes.into_iter().next().expect("one outcome"))
    }

    /// Plays the object route: checks the URL signature, then writes durably.
    pub fn put_through_url(&self, url: &SignedUrl, pack: PackId, bytes: &[u8]) -> Result<()> {
        let (exp, sig) = query_of(&url.url);
        let store = self.engine.object_store();
        store
            .verify_url(
                &UrlRequest {
                    method: "PUT",
                    cache: self.cache,
                    pack,
                    expires_at: exp,
                    content_length: Some(bytes.len() as u64),
                    signature: &sig,
                },
                oxabl_cache_core::ports::Clock::now(self.engine.clock()),
            )
            .map_err(|rejection| Error::bad_request(format!("url refused: {rejection:?}")))?;
        let mut writer = store.writer(&ObjectKey {
            cache: self.cache,
            pack,
        })?;
        writer.write_all(bytes)?;
        writer.finish(Some(bytes.len() as u64))?;
        Ok(())
    }

    /// Reserves and uploads; returns the pack id.
    pub async fn upload(&self, caller: &Caller, kind: NamespaceKind, built: &Built) -> PackId {
        match self.reserve(caller, kind, built).await.expect("reserve") {
            UploadOutcome::Reserved { pack, url } => {
                self.put_through_url(&url, pack, &built.bytes).expect("put");
                pack
            }
            other => panic!("expected a reservation, got {other:?}"),
        }
    }

    pub fn request(
        &self,
        pack: PackId,
        kind: NamespaceKind,
        built: &Built,
        entries: Vec<EntrySpec>,
    ) -> CommitRequest {
        let digest =
            *blake3::hash(format!("{:?}{:?}", built.segments, entries).as_bytes()).as_bytes();
        CommitRequest {
            cache: self.cache,
            pack,
            kind,
            digest,
            pack_size: built.bytes.len() as u64,
            segments: built.segments.clone(),
            entries,
        }
    }

    pub async fn commit(
        &self,
        caller: &Caller,
        pack: PackId,
        kind: NamespaceKind,
        built: &Built,
        entries: Vec<EntrySpec>,
    ) -> Result<Vec<EntryStatus>> {
        self.engine
            .commit_pack(caller, self.request(pack, kind, built, entries))
            .await
    }

    /// Upload and commit in one step.
    pub async fn publish(
        &self,
        caller: &Caller,
        kind: NamespaceKind,
        built: &Built,
        entries: Vec<EntrySpec>,
    ) -> (PackId, Vec<EntryStatus>) {
        let pack = self.upload(caller, kind, built).await;
        let statuses = self
            .commit(caller, pack, kind, built, entries)
            .await
            .expect("commit");
        (pack, statuses)
    }

    pub async fn fetch(
        &self,
        caller: &Caller,
        seeds: &[u32],
        urls: bool,
    ) -> oxabl_cache_core::usecases::LookupResponse {
        self.engine
            .lookup(
                caller,
                LookupRequest {
                    cache: self.cache,
                    namespaces: NamespaceSet {
                        trusted: true,
                        personal: true,
                    },
                    ids: seeds.iter().map(|seed| lookup_id(*seed)).collect(),
                    records: true,
                    urls,
                },
            )
            .await
            .expect("lookup")
    }

    pub async fn hits(&self, caller: &Caller, seeds: &[u32], records: bool) -> Vec<usize> {
        let response = self
            .engine
            .lookup(
                caller,
                LookupRequest {
                    cache: self.cache,
                    namespaces: NamespaceSet {
                        trusted: true,
                        personal: true,
                    },
                    ids: seeds.iter().map(|seed| lookup_id(*seed)).collect(),
                    records,
                    urls: false,
                },
            )
            .await
            .expect("lookup");
        response.hits.iter().map(|hit| hit.index).collect()
    }
}

fn getrandom_fill(bytes: &mut [u8; 16]) {
    let id = oxabl_cache_core::ids::NodeId::random();
    bytes.copy_from_slice(id.as_bytes());
}

fn query_of(url: &str) -> (i64, String) {
    let query = url.split_once('?').expect("url has a query").1;
    let mut exp = None;
    let mut sig = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("exp", value)) => exp = value.parse().ok(),
            Some(("sig", value)) => sig = Some(value.to_owned()),
            _ => {}
        }
    }
    (exp.expect("exp"), sig.expect("sig"))
}
