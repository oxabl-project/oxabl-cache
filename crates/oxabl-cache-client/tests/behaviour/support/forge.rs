//! A producer that holds real keys and breaks the rules: it writes records the way a hostile
//! or buggy member would, straight over HTTP.

use std::io::Cursor;

use oxabl_cache_crypto::{
    EntryFields, FileEntry, Identity, RecordContext, SealOptions, seal_record, seal_segment,
};
use oxabl_cache_format::{
    CacheId, CommitEntry, CommitRequest, CommitSegment, Limits, Namespace, PackFooter, PackHeader,
    PackSegment, SegmentKind,
};
use serde_json::json;

use super::fixture::Set;
use super::world::World;

/// Where and how a forged set is written.
pub struct Forgery<'a> {
    pub world: &'a World,
    /// Whose root key seals the set.
    pub holder: &'a Identity,
    /// Who signs the record.
    pub signer: &'a Identity,
    /// The namespace named in the record and in the commit.
    pub namespace: Namespace,
    /// The token that uploads.
    pub token: &'a str,
    /// The cache the pack is uploaded to.
    pub target: &'a str,
    /// The cache id sealed into the record.
    pub record_cache: CacheId,
    /// Replaces the paths of the set in the record, if set.
    pub paths: Option<Vec<String>>,
}

impl Forgery<'_> {
    /// Seals and uploads one set per entry and returns the commit statuses.
    pub async fn publish(&self, sets: &[Set]) -> Vec<oxabl_cache_format::CommitStatus> {
        let view = self.world.verified_roster();
        let root = view
            .unwrap_root_key(self.holder)
            .expect("the holder is a member");
        let limits = Limits::default();
        let target = CacheId::from_text(self.target).expect("cache id");
        let mut object = PackHeader { cache_id: target }.encode().to_vec();
        let mut index = Vec::new();
        let mut records = Vec::new();
        for set in sets {
            let stream = Cursor::new(
                set.files
                    .iter()
                    .flat_map(|f| f.bytes.iter().copied())
                    .collect::<Vec<u8>>(),
            );
            let mut segment = Vec::new();
            let seal = seal_segment(
                &root.segment_key(),
                stream,
                &SealOptions::default(),
                &limits,
                &mut segment,
            )
            .expect("seal");
            let lookup_id = root.lookup_key().lookup_id(&set.key.0);
            let paths: Vec<String> = self
                .paths
                .clone()
                .unwrap_or_else(|| set.files.iter().map(|f| f.path.clone()).collect());
            let fields = EntryFields {
                segment_id: seal.id,
                segment_len: seal.sealed_len as u32,
                dict_id: None,
                plain_len: seal.plain_len,
                created_at: 1_793_000_000,
                roster_version: view.version(),
                files: paths
                    .iter()
                    .zip(set.files.iter().map(|f| f.bytes.len() as u64))
                    .map(|(path, size)| FileEntry {
                        path: path.clone(),
                        size,
                    })
                    .collect(),
            };
            let record = seal_record(
                &root.record_key(),
                &RecordContext {
                    cache_id: self.record_cache,
                    epoch: view.epoch(),
                    namespace: self.namespace,
                    lookup_id,
                },
                &fields,
                self.signer,
                &limits,
            )
            .expect("record");
            index.push(PackSegment {
                segment_id: seal.id,
                offset: object.len() as u64,
                length: segment.len() as u32,
            });
            object.extend_from_slice(&segment);
            records.push((lookup_id, record));
        }
        let footer = PackFooter { segments: index };
        let trailer = footer
            .trailer(|b| *blake3::hash(b).as_bytes())
            .expect("trailer");
        object.extend_from_slice(&footer.encode().expect("footer"));
        object.extend_from_slice(&trailer.encode().expect("trailer"));
        let frame = CommitRequest {
            ns: self.namespace,
            pack_size: object.len() as u64,
            segments: footer
                .segments
                .iter()
                .map(|s| CommitSegment {
                    segment_id: s.segment_id,
                    offset: s.offset,
                    length: s.length,
                    kind: SegmentKind::Artifact,
                })
                .collect(),
            entries: records
                .iter()
                .enumerate()
                .map(|(i, (lookup_id, record))| CommitEntry {
                    lookup_id: *lookup_id,
                    segment_idx: i as u32,
                    dict_id: None,
                    epoch: view.epoch(),
                    record,
                })
                .collect(),
        }
        .encode()
        .expect("commit frame");
        self.upload(object, frame).await
    }

    async fn upload(
        &self,
        object: Vec<u8>,
        frame: Vec<u8>,
    ) -> Vec<oxabl_cache_format::CommitStatus> {
        let http = reqwest::Client::new();
        let base = format!("http://{}", self.world.server.address);
        let namespace = match self.namespace {
            Namespace::Trusted => "trusted",
            Namespace::Personal => "personal",
        };
        let mut client_ref = [0u8; 16];
        getrandom::fill(&mut client_ref).expect("random");
        let reserved: serde_json::Value = http
            .post(format!("{base}/v1/caches/{}/uploads", self.target))
            .bearer_auth(self.token)
            .json(&json!({"namespace": namespace, "packs": [{
                "client_ref": client_ref.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                "size": object.len(),
            }]}))
            .send()
            .await
            .expect("reserve")
            .json()
            .await
            .expect("json");
        let slot = &reserved["packs"][0];
        assert_eq!(slot["status"], "ok", "{reserved}");
        let put = http
            .put(slot["url"].as_str().expect("url"))
            .body(object)
            .send()
            .await
            .expect("put");
        assert!(put.status().is_success());
        let pack = slot["pack_id"].as_str().expect("pack");
        let committed = http
            .post(format!(
                "{base}/v1/caches/{}/packs/{pack}/commit",
                self.target
            ))
            .bearer_auth(self.token)
            .header("content-type", "application/vnd.oxabl-cache.v1")
            .body(frame)
            .send()
            .await
            .expect("commit");
        assert_eq!(
            committed.status().as_u16(),
            200,
            "{:?}",
            committed.text().await
        );
        oxabl_cache_format::CommitResponse::decode(&committed.bytes().await.expect("bytes"))
            .expect("OXCA")
            .statuses
    }
}

impl Forgery<'_> {
    /// Publishes `dictionary` as the cache's dictionary: a pack with one dictionary segment and
    /// no entries, then a pointer signed by `signer`.
    pub async fn publish_dictionary(&self, dictionary: &[u8]) {
        let view = self.world.verified_roster();
        let root = view
            .unwrap_root_key(self.holder)
            .expect("the holder is a member");
        let mut segment = Vec::new();
        let seal =
            oxabl_cache_crypto::seal_dictionary(&root.segment_key(), dictionary, &mut segment)
                .expect("seal");
        let target = CacheId::from_text(self.target).expect("cache id");
        let mut object = PackHeader { cache_id: target }.encode().to_vec();
        let footer = PackFooter {
            segments: vec![PackSegment {
                segment_id: seal.id,
                offset: object.len() as u64,
                length: segment.len() as u32,
            }],
        };
        object.extend_from_slice(&segment);
        let trailer = footer
            .trailer(|b| *blake3::hash(b).as_bytes())
            .expect("trailer");
        object.extend_from_slice(&footer.encode().expect("footer"));
        object.extend_from_slice(&trailer.encode().expect("trailer"));
        let frame = CommitRequest {
            ns: Namespace::Trusted,
            pack_size: object.len() as u64,
            segments: vec![CommitSegment {
                segment_id: seal.id,
                offset: footer.segments[0].offset,
                length: footer.segments[0].length,
                kind: SegmentKind::Dictionary,
            }],
            entries: Vec::new(),
        }
        .encode()
        .expect("commit frame");
        self.upload(object, frame).await;
        let pointer =
            oxabl_cache_crypto::sign_pointer(self.signer, target, view.epoch(), 1, seal.id);
        let put = reqwest::Client::new()
            .put(format!(
                "http://{}/v1/caches/{}/dictionary",
                self.world.server.address, self.target
            ))
            .bearer_auth(self.token)
            .header("If-Match", "\"v0\"")
            .header("content-type", "application/vnd.oxabl-cache.v1")
            .body(pointer.encode())
            .send()
            .await
            .expect("put pointer");
        assert_eq!(put.status().as_u16(), 204, "{:?}", put.text().await);
    }
}
