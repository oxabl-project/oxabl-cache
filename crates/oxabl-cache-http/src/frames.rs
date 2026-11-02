//! Conversions between the byte formats and the core's domain types. Every frame
//! is parsed by the format crate; records and segments stay opaque bytes.

use crate::error::ApiError;
use oxabl_cache_core::domain::entry::{EntrySpec, EntryStatus};
use oxabl_cache_core::domain::namespace::NamespaceKind;
use oxabl_cache_core::domain::roster::{
    DictionaryPointer, MemberRole, PublicKey, Roster, RosterMember, Signature,
};
use oxabl_cache_core::domain::segment::{SegmentKind, SegmentSpec};
use oxabl_cache_core::ids::{CacheId, LookupId, PackId, SegmentId};
use oxabl_cache_core::usecases::{CommitRequest, LookupRequest, LookupResponse, NamespaceSet};
use oxabl_cache_format as format;

const ROSTER_DOMAIN: &[u8] = b"oxabl-cache v1 roster";
const DICTIONARY_DOMAIN: &[u8] = b"oxabl-cache v1 dictionary";

fn namespace_kind(ns: format::Namespace) -> NamespaceKind {
    match ns {
        format::Namespace::Trusted => NamespaceKind::Trusted,
        format::Namespace::Personal => NamespaceKind::Personal,
    }
}

fn wire_namespace(kind: NamespaceKind) -> format::Namespace {
    match kind {
        NamespaceKind::Trusted => format::Namespace::Trusted,
        NamespaceKind::Personal => format::Namespace::Personal,
    }
}

/// A lookup request frame, with the shape its response must have.
pub(crate) struct ParsedLookup {
    pub request: LookupRequest,
    pub shape: format::LookupShape,
}

pub(crate) fn parse_lookup(cache: CacheId, body: &[u8]) -> Result<ParsedLookup, ApiError> {
    let frame = format::LookupRequest::decode(body)?;
    let shape = frame.shape()?;
    Ok(ParsedLookup {
        request: LookupRequest {
            cache,
            namespaces: NamespaceSet {
                trusted: frame.namespaces.trusted,
                personal: frame.namespaces.personal,
            },
            ids: frame
                .ids
                .iter()
                .map(|id| LookupId::from_bytes(id.0))
                .collect(),
            records: frame.flags.records,
            urls: frame.flags.urls,
        },
        shape,
    })
}

/// The bytes of a lookup response.
pub(crate) fn encode_lookup(
    shape: &format::LookupShape,
    answer: &LookupResponse,
) -> Result<Vec<u8>, ApiError> {
    let flags = shape.flags;
    let packs = answer
        .packs
        .iter()
        .map(|locator| format::ResponsePack {
            pack_id: format::PackId(*locator.pack.as_bytes()),
            url: locator.url.as_ref().map_or("", |url| url.url.as_str()),
            expires_at: locator
                .url
                .as_ref()
                .map_or(0, |url| u64::try_from(url.expires_at).unwrap_or(0)),
        })
        .collect();
    let hits = answer
        .hits
        .iter()
        .map(|hit| {
            let record = match (&hit.location, &hit.record) {
                (Some(location), Some(record)) => Some(format::HitRecord {
                    pack_idx: u32::try_from(location.pack_index).unwrap_or(u32::MAX),
                    offset: location.offset,
                    length: location.length,
                    record,
                }),
                _ => None,
            };
            format::LookupHit {
                index: u32::try_from(hit.index).unwrap_or(u32::MAX),
                ns: wire_namespace(hit.namespace),
                record,
            }
        })
        .collect();
    let dictionaries = answer
        .dictionaries
        .iter()
        .map(|dictionary| format::ResponseDictionary {
            dict_id: format::SegmentId(*dictionary.id.as_bytes()),
            pack_idx: u32::try_from(dictionary.location.pack_index).unwrap_or(u32::MAX),
            offset: dictionary.location.offset,
            length: dictionary.location.length,
        })
        .collect();
    let frame = format::LookupResponse {
        flags,
        packs,
        hits,
        dictionaries,
    };
    Ok(frame.encode(shape)?)
}

/// The number of request ids that hit in at least one namespace.
pub(crate) fn distinct_hits(answer: &LookupResponse) -> usize {
    let mut indexes: Vec<usize> = answer.hits.iter().map(|hit| hit.index).collect();
    indexes.dedup();
    indexes.len()
}

fn segment_kind(kind: format::SegmentKind) -> SegmentKind {
    match kind {
        format::SegmentKind::Artifact => SegmentKind::Artifact,
        format::SegmentKind::Dictionary => SegmentKind::Dictionary,
    }
}

/// A commit frame as the core wants it.
pub(crate) fn parse_commit(
    cache: CacheId,
    pack: PackId,
    body: &[u8],
) -> Result<CommitRequest, ApiError> {
    let frame = format::CommitRequest::decode(body)?;
    Ok(CommitRequest {
        cache,
        pack,
        kind: namespace_kind(frame.ns),
        digest: *blake3::hash(body).as_bytes(),
        pack_size: frame.pack_size,
        segments: frame
            .segments
            .iter()
            .map(|segment| SegmentSpec {
                id: SegmentId::from_bytes(segment.segment_id.0),
                offset: segment.offset,
                length: segment.length,
                kind: segment_kind(segment.kind),
            })
            .collect(),
        entries: frame
            .entries
            .iter()
            .map(|entry| EntrySpec {
                lookup_id: LookupId::from_bytes(entry.lookup_id.0),
                segment_index: entry.segment_idx,
                dict_id: entry.dict_id.map(|id| SegmentId::from_bytes(id.0)),
                epoch: entry.epoch,
                record: entry.record.to_vec(),
                header_ok: format::SealedRecord::decode(entry.record).is_ok(),
            })
            .collect(),
    })
}

/// The bytes of a commit response.
pub(crate) fn encode_statuses(statuses: &[EntryStatus]) -> Result<Vec<u8>, ApiError> {
    let statuses = statuses
        .iter()
        .map(|status| match status {
            EntryStatus::Committed => format::CommitStatus::Committed,
            EntryStatus::Exists => format::CommitStatus::Exists,
            EntryStatus::Invalid => format::CommitStatus::Invalid,
            EntryStatus::MissingDictionary => format::CommitStatus::MissingDictionary,
        })
        .collect();
    Ok(format::CommitResponse { statuses }.encode()?)
}

fn role(role: format::Role) -> MemberRole {
    match role {
        format::Role::Owner => MemberRole::Owner,
        format::Role::Ci => MemberRole::Ci,
        format::Role::Developer => MemberRole::Developer,
        format::Role::Reader => MemberRole::Reader,
    }
}

/// A roster frame for `cache`; a frame naming another cache is refused.
pub(crate) fn parse_roster(cache: CacheId, body: &[u8]) -> Result<Roster, ApiError> {
    let frame = format::Roster::decode(body)?;
    if frame.cache_id.0 != *cache.as_bytes() {
        return Err(ApiError::bad_request("the roster names another cache"));
    }
    let mut signed_message = ROSTER_DOMAIN.to_vec();
    signed_message.extend_from_slice(&frame.signed_bytes()?);
    Ok(Roster {
        version: frame.version,
        epoch: frame.epoch,
        bytes: body.to_vec(),
        members: frame
            .members
            .iter()
            .map(|member| RosterMember {
                role: role(member.role),
                sign_keys: member.sign_keys.clone(),
            })
            .collect(),
        signed_message,
        signatures: frame
            .signatures
            .iter()
            .map(|signature| Signature {
                key: signature.owner_key,
                signature: signature.signature,
            })
            .collect(),
    })
}

/// A dictionary pointer frame for `cache`; a frame naming another cache is refused.
pub(crate) fn parse_pointer(cache: CacheId, body: &[u8]) -> Result<DictionaryPointer, ApiError> {
    let frame = format::DictionaryPointer::decode(body)?;
    if frame.cache_id.0 != *cache.as_bytes() {
        return Err(ApiError::bad_request("the pointer names another cache"));
    }
    let mut signed_message = DICTIONARY_DOMAIN.to_vec();
    signed_message.extend_from_slice(&frame.signed_bytes());
    let signer: PublicKey = frame.signer;
    Ok(DictionaryPointer {
        version: frame.version,
        epoch: frame.epoch,
        segment_id: SegmentId::from_bytes(frame.dict_segment_id.0),
        bytes: body.to_vec(),
        signed_message,
        signature: Signature {
            key: signer,
            signature: frame.signature,
        },
    })
}
