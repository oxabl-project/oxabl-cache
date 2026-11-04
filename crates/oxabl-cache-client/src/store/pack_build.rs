use bytes::Bytes;
use oxabl_cache_format::limits::{COMMIT_ENTRIES, PACK_BYTES, PACK_SEGMENTS};
use oxabl_cache_format::pack_footer::FOOTER_ENTRY_LEN;
use oxabl_cache_format::pack_header::PACK_HEADER_LEN;
use oxabl_cache_format::pack_trailer::PACK_TRAILER_LEN;
use oxabl_cache_format::{
    CacheId, CommitEntry, CommitRequest, CommitSegment, Namespace, PackFooter, PackHeader,
    PackSegment, SegmentId, SegmentKind,
};

use super::seal::Sealed;
use super::types::StoreFailure;

/// Bytes a pack of `segments` segments adds around its segment bytes.
pub(crate) fn framing(segments: usize) -> u64 {
    (PACK_HEADER_LEN + segments * FOOTER_ENTRY_LEN + PACK_TRAILER_LEN) as u64
}

/// A pack object and the exact commit frame that names its entries. Both are built once and
/// sent as they are, however often a request is repeated.
pub(crate) struct BuiltPack {
    pub object: Bytes,
    pub commit: Bytes,
}

/// The pack object holding `segments` in order, and its footer.
fn assemble(
    cache: CacheId,
    segments: &[(SegmentId, &[u8])],
) -> Result<(Vec<u8>, PackFooter), StoreFailure> {
    let mut object = PackHeader { cache_id: cache }.encode().to_vec();
    let mut index = Vec::with_capacity(segments.len());
    for (segment_id, bytes) in segments {
        index.push(PackSegment {
            segment_id: *segment_id,
            offset: object.len() as u64,
            length: u32::try_from(bytes.len()).map_err(|_| StoreFailure::TooLarge)?,
        });
        object.extend_from_slice(bytes);
    }
    let footer = PackFooter { segments: index };
    let trailer = footer
        .trailer(|bytes| *blake3::hash(bytes).as_bytes())
        .map_err(|_| StoreFailure::TooLarge)?;
    object.extend_from_slice(&footer.encode().map_err(|_| StoreFailure::TooLarge)?);
    object.extend_from_slice(&trailer.encode().map_err(|_| StoreFailure::TooLarge)?);
    Ok((object, footer))
}

/// A trusted pack that holds one sealed dictionary segment and names no entry.
pub(crate) fn build_dictionary(
    cache: CacheId,
    segment_id: SegmentId,
    segment: &[u8],
) -> Result<BuiltPack, StoreFailure> {
    let (object, footer) = assemble(cache, &[(segment_id, segment)])?;
    let commit = CommitRequest {
        ns: Namespace::Trusted,
        pack_size: object.len() as u64,
        segments: footer
            .segments
            .iter()
            .map(|s| CommitSegment {
                segment_id: s.segment_id,
                offset: s.offset,
                length: s.length,
                kind: SegmentKind::Dictionary,
            })
            .collect(),
        entries: Vec::new(),
    }
    .encode()
    .map_err(|_| StoreFailure::TooLarge)?;
    Ok(BuiltPack {
        object: Bytes::from(object),
        commit: Bytes::from(commit),
    })
}

pub(crate) fn build(
    cache: CacheId,
    namespace: Namespace,
    epoch: u32,
    sealed: &[&Sealed],
) -> Result<BuiltPack, StoreFailure> {
    if sealed.len() > PACK_SEGMENTS.min(COMMIT_ENTRIES) {
        return Err(StoreFailure::TooLarge);
    }
    let body: u64 = sealed.iter().map(|s| s.segment.len() as u64).sum();
    if body + framing(sealed.len()) > PACK_BYTES {
        return Err(StoreFailure::TooLarge);
    }
    let segments: Vec<(SegmentId, &[u8])> = sealed
        .iter()
        .map(|s| (s.segment_id, s.segment.as_slice()))
        .collect();
    let (object, footer) = assemble(cache, &segments)?;

    let commit = CommitRequest {
        ns: namespace,
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
        entries: sealed
            .iter()
            .enumerate()
            .map(|(i, s)| CommitEntry {
                lookup_id: s.lookup_id,
                segment_idx: i as u32,
                dict_id: s.dict_id,
                epoch,
                record: &s.record,
            })
            .collect(),
    }
    .encode()
    .map_err(|_| StoreFailure::TooLarge)?;
    Ok(BuiltPack {
        object: Bytes::from(object),
        commit: Bytes::from(commit),
    })
}
