//! Builds valid frames from arbitrary bytes, then checks that they encode, decode back to the
//! same value and re-encode to the same bytes.

use std::collections::HashSet;
use std::fmt::Debug;

use arbitrary::{Result, Unstructured};
use oxabl_cache_format::bech32::{self, IDENTITY_HRP};
use oxabl_cache_format::limits::{COMMIT_ENTRIES, FILES_PER_SET, PACK_BYTES};
use oxabl_cache_format::padme::padme;
use oxabl_cache_format::segment::{CHUNK_PLAINTEXT_LEN, CHUNK_TAG_LEN, MIN_SEGMENT_LEN};
use oxabl_cache_format::{
    ApiToken, CacheId, Codec, CommitEntry, CommitRequest, CommitResponse, CommitSegment,
    CommitStatus, DictionaryPointer, EntryFile, EntryPayload, HitRecord, Limits, LookupFlags,
    LookupHit, LookupId, LookupRequest, LookupResponse, Member, MemberDescriptor, Namespace,
    Namespaces, PackFooter, PackHeader, PackId, PackSegment, PackTrailer, RequestId,
    ResponseDictionary, ResponsePack, Role, Roster, RosterSignature, SealedRecord, SealedSegment,
    SegmentHeader, SegmentId, SegmentKind, SegmentPlaintext, TenantId,
};

use crate::blake3;

/// Decodes `$encoded` to `$original`, and rejects it with one trailing byte appended.
macro_rules! roundtrip {
    ($encoded:expr, $original:expr, |$b:ident| $decode:expr) => {{
        let encoded: &[u8] = &$encoded;
        let $b = encoded;
        assert_eq!($decode.as_ref(), Ok(&$original));
        let mut longer = encoded.to_vec();
        longer.push(0);
        let $b = longer.as_slice();
        assert!($decode.is_err(), "a trailing byte is accepted");
    }};
}

fn decodes_to<T: PartialEq + Debug>(decoded: oxabl_cache_format::Result<T>, original: &T) {
    assert_eq!(decoded.as_ref(), Ok(original));
}

fn bytes(u: &mut Unstructured<'_>, max: usize) -> Result<Vec<u8>> {
    let len = u.int_in_range(0..=max)?;
    Ok(u.bytes(len.min(u.len()))?.to_vec())
}

fn array<const N: usize>(u: &mut Unstructured<'_>) -> Result<[u8; N]> {
    let mut out = [0u8; N];
    for b in &mut out {
        *b = u.arbitrary()?;
    }
    Ok(out)
}

fn nonzero32(u: &mut Unstructured<'_>) -> Result<[u8; 32]> {
    let mut v: [u8; 32] = array(u)?;
    v[0] |= 1;
    Ok(v)
}

fn text(u: &mut Unstructured<'_>, max: usize) -> Result<String> {
    let s: String = u.arbitrary()?;
    Ok(s.chars().take(max).collect())
}

fn namespaces(u: &mut Unstructured<'_>) -> Result<Namespaces> {
    Ok(match u.int_in_range(0..=2)? {
        0 => Namespaces {
            trusted: true,
            personal: false,
        },
        1 => Namespaces {
            trusted: false,
            personal: true,
        },
        _ => Namespaces {
            trusted: true,
            personal: true,
        },
    })
}

fn flags(u: &mut Unstructured<'_>) -> Result<LookupFlags> {
    Ok(LookupFlags {
        records: u.arbitrary()?,
        urls: u.arbitrary()?,
    })
}

fn lookup(u: &mut Unstructured<'_>) -> Result<()> {
    let ids = (0..u.int_in_range(0..=40)?)
        .map(|_| array(u).map(LookupId))
        .collect::<Result<Vec<_>>>()?;
    let request = LookupRequest {
        flags: flags(u)?,
        namespaces: namespaces(u)?,
        ids,
    };
    let encoded = request.encode().unwrap();
    roundtrip!(encoded, request, |b| LookupRequest::decode(b));

    let shape = request.shape().unwrap();
    let cap = shape.count as usize * shape.namespaces.len();
    let pack_count = if cap == 0 {
        0
    } else {
        u.int_in_range(1..=(2 * cap).min(6))?
    };
    let urls = (0..pack_count)
        .map(|_| text(u, 40).map(|t| format!("u{t}")))
        .collect::<Result<Vec<_>>>()?;
    let packs = urls
        .iter()
        .map(|url| {
            Ok(ResponsePack {
                pack_id: PackId(array(u)?),
                url: if shape.flags.urls { url } else { "" },
                expires_at: u.arbitrary()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let records = (0..cap.min(12))
        .map(|_| bytes(u, 400))
        .collect::<Result<Vec<_>>>()?;
    let mut seen = HashSet::new();
    let mut hits = Vec::new();
    for record in &records {
        let index = u.int_in_range(0..=shape.count - 1)?;
        let ns = if shape.namespaces.trusted && (!shape.namespaces.personal || u.arbitrary()?) {
            Namespace::Trusted
        } else {
            Namespace::Personal
        };
        if !seen.insert((index, ns)) {
            continue;
        }
        let record = if shape.flags.records {
            Some(HitRecord {
                pack_idx: u.int_in_range(0..=pack_count as u32 - 1)?,
                offset: u.int_in_range(24..=1 << 20)?,
                length: u.int_in_range(MIN_SEGMENT_LEN as u32..=1 << 20)?,
                record,
            })
        } else {
            None
        };
        hits.push(LookupHit { index, ns, record });
    }
    let mut dictionaries = Vec::new();
    if shape.flags.records && pack_count > 0 {
        let mut dicts = HashSet::new();
        for _ in 0..u.int_in_range(0..=cap.min(4))? {
            let dict_id = SegmentId(array(u)?);
            if dicts.insert(dict_id) {
                dictionaries.push(ResponseDictionary {
                    dict_id,
                    pack_idx: u.int_in_range(0..=pack_count as u32 - 1)?,
                    offset: u.int_in_range(24..=1 << 20)?,
                    length: u.int_in_range(MIN_SEGMENT_LEN as u32..=1 << 20)?,
                });
            }
        }
    }
    let response = LookupResponse {
        flags: shape.flags,
        packs,
        hits,
        dictionaries,
    };
    let encoded = response.encode(&shape).unwrap();
    roundtrip!(encoded, response, |b| LookupResponse::decode(b, &shape));
    Ok(())
}

fn commit(u: &mut Unstructured<'_>) -> Result<()> {
    let ns = if u.arbitrary()? {
        Namespace::Trusted
    } else {
        Namespace::Personal
    };
    let segments = (0..u.int_in_range(0..=8)?)
        .map(|_| {
            let kind = if ns == Namespace::Trusted && u.arbitrary()? {
                SegmentKind::Dictionary
            } else {
                SegmentKind::Artifact
            };
            Ok(CommitSegment {
                segment_id: SegmentId(array(u)?),
                offset: u.arbitrary()?,
                length: u.arbitrary()?,
                kind,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let records = (0..u.int_in_range(0..=COMMIT_ENTRIES.min(10))?)
        .map(|_| bytes(u, 600))
        .collect::<Result<Vec<_>>>()?;
    let entries = records
        .iter()
        .map(|record| {
            Ok(CommitEntry {
                lookup_id: LookupId(array(u)?),
                segment_idx: u.arbitrary()?,
                dict_id: if u.arbitrary()? {
                    Some(SegmentId(nonzero32(u)?))
                } else {
                    None
                },
                epoch: u.arbitrary()?,
                record,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let request = CommitRequest {
        ns,
        pack_size: u.int_in_range(0..=PACK_BYTES)?,
        segments,
        entries,
    };
    let encoded = request.encode().unwrap();
    roundtrip!(encoded, request, |b| CommitRequest::decode(b));

    let statuses = (0..u.int_in_range(0..=50)?)
        .map(|_| {
            Ok(match u.int_in_range(0..=3)? {
                0 => CommitStatus::Committed,
                1 => CommitStatus::Exists,
                2 => CommitStatus::Invalid,
                _ => CommitStatus::MissingDictionary,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let response = CommitResponse { statuses };
    let encoded = response.encode().unwrap();
    roundtrip!(encoded, response, |b| CommitResponse::decode(b));
    Ok(())
}

fn pack(u: &mut Unstructured<'_>) -> Result<()> {
    let header = PackHeader {
        cache_id: CacheId(array(u)?),
    };
    let encoded = header.encode();
    roundtrip!(encoded, header, |b| PackHeader::decode(b));

    let mut offset = 24u64;
    let mut segments = Vec::new();
    for _ in 0..u.int_in_range(0..=30)? {
        let length = u.int_in_range(MIN_SEGMENT_LEN as u32..=70_000)?;
        segments.push(PackSegment {
            segment_id: SegmentId(array(u)?),
            offset,
            length,
        });
        offset += u64::from(length);
    }
    let footer = PackFooter { segments };
    let encoded = footer.encode().unwrap();
    let trailer = footer.trailer(blake3).unwrap();
    let tail = trailer.encode().unwrap();
    let object_size = trailer.object_size().unwrap();
    assert_eq!(object_size, offset + encoded.len() as u64 + 48);
    roundtrip!(tail, trailer, |b| PackTrailer::decode(b, object_size));
    decodes_to(PackFooter::decode(&encoded, &trailer, blake3), &footer);
    Ok(())
}

fn segment(u: &mut Unstructured<'_>) -> Result<()> {
    let header = SegmentHeader {
        salt: array(u)?,
        k_commit: array(u)?,
    };
    let encoded = header.encode();
    roundtrip!(encoded, header, |b| SegmentHeader::decode(b));

    let plaintext = u.int_in_range(1..=3 * CHUNK_PLAINTEXT_LEN)?;
    let body = vec![
        u.arbitrary::<u8>()?;
        plaintext + plaintext.div_ceil(CHUNK_PLAINTEXT_LEN) * CHUNK_TAG_LEN
    ];
    let segment = SealedSegment {
        header,
        body: &body,
    };
    let encoded = segment.encode().unwrap();
    decodes_to(SealedSegment::decode(&encoded), &segment);

    let compressed = bytes(u, 3000)?;
    let codec = [Codec::None, Codec::Zstd, Codec::ZstdDictionary][u.int_in_range(0..=2)?];
    let plain = SegmentPlaintext {
        codec,
        compressed: &compressed,
    };
    let encoded = plain.encode().unwrap();
    roundtrip!(encoded, plain, |b| SegmentPlaintext::decode(b));

    let len: u64 = u.int_in_range(0..=u64::MAX / 2)?;
    let padded = padme(len).unwrap();
    assert!(padded >= len);
    assert_eq!(padme(padded), Some(padded));
    Ok(())
}

fn record(u: &mut Unstructured<'_>) -> Result<()> {
    let names = (0..u.int_in_range(0..=FILES_PER_SET)?)
        .map(|i| text(u, 30).map(|t| format!("d{i}/f{}", t.replace(['/', '\0'], "_"))))
        .collect::<Result<Vec<_>>>()?;
    let files: Vec<EntryFile<'_>> = names
        .iter()
        .map(|path| {
            Ok(EntryFile {
                path,
                size: u.int_in_range(0..=1 << 20)?,
            })
        })
        .collect::<Result<_>>()?;
    let payload = EntryPayload {
        segment_id: SegmentId(array(u)?),
        segment_len: u.int_in_range(MIN_SEGMENT_LEN as u32..=PACK_BYTES as u32)?,
        dict_id: if u.arbitrary()? {
            Some(SegmentId(nonzero32(u)?))
        } else {
            None
        },
        plain_len: files.iter().map(|f| f.size).sum(),
        created_at: u.arbitrary()?,
        roster_version: u.arbitrary()?,
        signer: array(u)?,
        files,
        signature: array(u)?,
    };
    let limits = Limits::default();
    let plaintext = payload.encode(&limits).unwrap();
    roundtrip!(plaintext, payload, |b| EntryPayload::decode(b, &limits));
    assert!(payload.signed_bytes(&limits).unwrap().len() + 64 <= plaintext.len());

    let mut ciphertext = plaintext;
    ciphertext.extend_from_slice(&array::<16>(u)?);
    let record = SealedRecord {
        salt: array(u)?,
        k_commit: array(u)?,
        ciphertext: &ciphertext,
    };
    let encoded = record.encode().unwrap();
    roundtrip!(encoded, record, |b| SealedRecord::decode(b));
    Ok(())
}

fn roster(u: &mut Unstructured<'_>) -> Result<()> {
    let labels = (0..u.int_in_range(0..=4)?)
        .map(|_| text(u, 30))
        .collect::<Result<Vec<_>>>()?;
    let recipients = (0..8).map(|_| text(u, 70)).collect::<Result<Vec<_>>>()?;
    let members = labels
        .iter()
        .map(|label| {
            let role =
                [Role::Owner, Role::Ci, Role::Developer, Role::Reader][u.int_in_range(0..=3)?];
            let sign_keys = (0..u.int_in_range(0..=3)?)
                .map(|_| array(u))
                .collect::<Result<_>>()?;
            let n = u.int_in_range(0..=3)?;
            Ok(Member {
                label,
                role,
                sign_keys,
                age_recipients: recipients.iter().take(n).map(String::as_str).collect(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let keyring = bytes(u, 500)?;
    let signatures = (0..u.int_in_range(1..=4)?)
        .map(|_| {
            Ok(RosterSignature {
                owner_key: array(u)?,
                signature: array(u)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let roster = Roster {
        cache_id: CacheId(array(u)?),
        version: u.arbitrary()?,
        epoch: u.arbitrary()?,
        members,
        keyring: &keyring,
        signatures,
    };
    let encoded = roster.encode().unwrap();
    roundtrip!(encoded, roster, |b| Roster::decode(b));
    assert!(encoded.starts_with(&roster.signed_bytes().unwrap()));
    Ok(())
}

fn small(u: &mut Unstructured<'_>) -> Result<()> {
    let pointer = DictionaryPointer {
        cache_id: CacheId(array(u)?),
        epoch: u.arbitrary()?,
        version: u.arbitrary()?,
        dict_segment_id: SegmentId(array(u)?),
        signer: array(u)?,
        signature: array(u)?,
    };
    let encoded = pointer.encode();
    roundtrip!(encoded, pointer, |b| DictionaryPointer::decode(b));
    assert!(encoded.starts_with(&pointer.signed_bytes()));

    let descriptor = MemberDescriptor {
        ed25519_public: array(u)?,
        x25519_public: array(u)?,
    };
    let text = descriptor.encode().unwrap();
    decodes_to(MemberDescriptor::decode(&text), &descriptor);

    let payload = bytes(u, 100)?;
    let text = bech32::encode(IDENTITY_HRP, &payload).unwrap();
    assert_eq!(bech32::decode(&text, IDENTITY_HRP), Ok(payload));

    let cache = CacheId(array(u)?);
    assert_eq!(CacheId::from_text(&cache.to_text()), Ok(cache));
    let tenant = TenantId(array(u)?);
    assert_eq!(TenantId::from_text(&tenant.to_text()), Ok(tenant));
    let request = RequestId(array(u)?);
    assert_eq!(RequestId::from_text(&request.to_text()), Ok(request));
    let token = ApiToken {
        id: array(u)?,
        secret: array(u)?,
    };
    assert_eq!(ApiToken::from_text(&token.to_text()), Ok(token));
    Ok(())
}

/// Runs one structured case chosen by the first byte of `u`.
pub fn run(u: &mut Unstructured<'_>) -> Result<()> {
    match u.int_in_range(0..=6)? {
        0 => lookup(u),
        1 => commit(u),
        2 => pack(u),
        3 => segment(u),
        4 => record(u),
        5 => roster(u),
        _ => small(u),
    }
}
