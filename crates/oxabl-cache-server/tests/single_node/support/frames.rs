//! Frames built with the format crate. Segments and records are random bytes of
//! valid outer shape: the server never opens them.

use oxabl_cache_format as format;

pub fn random(len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes).expect("randomness");
    bytes
}

pub fn lookup_id() -> [u8; 32] {
    let mut id = [0u8; 32];
    getrandom::fill(&mut id).expect("randomness");
    id
}

/// A sealed record header with a random body, 512 bytes long.
pub fn record() -> Vec<u8> {
    let mut salt = [0u8; 32];
    let mut k_commit = [0u8; 32];
    getrandom::fill(&mut salt).expect("randomness");
    getrandom::fill(&mut k_commit).expect("randomness");
    let ciphertext = random(512 - 72);
    format::SealedRecord {
        salt,
        k_commit,
        ciphertext: &ciphertext,
    }
    .encode()
    .expect("record")
}

pub struct Pack {
    pub bytes: Vec<u8>,
    pub segments: Vec<format::CommitSegment>,
    pub contents: Vec<Vec<u8>>,
}

/// An `OXPK` object holding random segments of the given sizes.
pub fn pack(cache: [u8; 16], sizes: &[usize]) -> Pack {
    let mut bytes = format::PackHeader {
        cache_id: format::CacheId(cache),
    }
    .encode()
    .to_vec();
    let mut index = Vec::new();
    let mut segments = Vec::new();
    let mut contents = Vec::new();
    for size in sizes {
        let segment = random(*size);
        let id = format::SegmentId(*blake3::hash(&segment).as_bytes());
        let offset = bytes.len() as u64;
        let length = u32::try_from(segment.len()).expect("small");
        index.push(format::PackSegment {
            segment_id: id,
            offset,
            length,
        });
        segments.push(format::CommitSegment {
            segment_id: id,
            offset,
            length,
            kind: format::SegmentKind::Artifact,
        });
        bytes.extend_from_slice(&segment);
        contents.push(segment);
    }
    let footer = format::PackFooter { segments: index };
    let trailer = footer
        .trailer(|data| *blake3::hash(data).as_bytes())
        .expect("trailer");
    bytes.extend_from_slice(&footer.encode().expect("footer"));
    bytes.extend_from_slice(&trailer.encode().expect("trailer"));
    Pack {
        bytes,
        segments,
        contents,
    }
}

/// An entry of a commit: lookup id, the segment it names and the epoch.
pub struct Entry {
    pub lookup: [u8; 32],
    pub segment: u32,
    pub epoch: u32,
    pub record: Vec<u8>,
}

impl Entry {
    pub fn new(lookup: [u8; 32], segment: u32) -> Self {
        Self {
            lookup,
            segment,
            epoch: 0,
            record: record(),
        }
    }
}

pub fn commit(ns: format::Namespace, pack: &Pack, entries: &[Entry]) -> Vec<u8> {
    format::CommitRequest {
        ns,
        pack_size: pack.bytes.len() as u64,
        segments: pack.segments.clone(),
        entries: entries
            .iter()
            .map(|entry| format::CommitEntry {
                lookup_id: format::LookupId(entry.lookup),
                segment_idx: entry.segment,
                dict_id: None,
                epoch: entry.epoch,
                record: &entry.record,
            })
            .collect(),
    }
    .encode()
    .expect("commit frame")
}

pub fn lookup(
    ids: &[[u8; 32]],
    records: bool,
    urls: bool,
    trusted: bool,
    personal: bool,
) -> (Vec<u8>, format::LookupShape) {
    let request = format::LookupRequest {
        flags: format::LookupFlags { records, urls },
        namespaces: format::Namespaces { trusted, personal },
        ids: ids.iter().map(|id| format::LookupId(*id)).collect(),
    };
    (
        request.encode().expect("lookup frame"),
        request.shape().expect("shape"),
    )
}

pub fn statuses(bytes: &[u8]) -> Vec<format::CommitStatus> {
    format::CommitResponse::decode(bytes)
        .expect("OXCA")
        .statuses
}

/// An `OXTL` frame of the given epoch around random bytes: the outer shape of a sealed
/// telemetry report, which is all the server can check.
pub fn telemetry(epoch: u32, sealed_len: usize) -> Vec<u8> {
    let mut frame = b"OXTL\x01\x00".to_vec();
    frame.extend_from_slice(&epoch.to_be_bytes());
    frame.extend_from_slice(&random(12));
    frame.extend_from_slice(&random(sealed_len));
    frame
}
