//! Byte vectors written from the field tables of `docs/protocol.md`. They test the published
//! format, not the code: each frame is built by hand, decoded, and every malformed variant of
//! it must be rejected with the matching error.

use oxabl_cache_format::bech32;
use oxabl_cache_format::limits;
use oxabl_cache_format::padme::padme;
use oxabl_cache_format::*;

#[derive(Clone, Default)]
struct B(Vec<u8>);

impl B {
    fn new() -> Self {
        Self::default()
    }
    fn raw(mut self, bytes: &[u8]) -> Self {
        self.0.extend_from_slice(bytes);
        self
    }
    fn u8(self, v: u8) -> Self {
        self.raw(&[v])
    }
    fn u16(self, v: u16) -> Self {
        self.raw(&v.to_be_bytes())
    }
    fn u32(self, v: u32) -> Self {
        self.raw(&v.to_be_bytes())
    }
    fn u64(self, v: u64) -> Self {
        self.raw(&v.to_be_bytes())
    }
    fn i64(self, v: i64) -> Self {
        self.raw(&v.to_be_bytes())
    }
    fn s16(self, s: &str) -> Self {
        self.u16(s.len() as u16).raw(s.as_bytes())
    }
    fn fill(self, byte: u8, n: usize) -> Self {
        self.raw(&vec![byte; n])
    }
    fn done(self) -> Vec<u8> {
        self.0
    }
}

fn head(magic: &[u8; 4], flags: u8) -> B {
    B::new().raw(magic).u8(1).u8(flags)
}

fn with(bytes: &[u8], offset: usize, value: u8) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out[offset] = value;
    out
}

fn plus(bytes: &[u8], extra: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out.extend_from_slice(extra);
    out
}

fn minus(bytes: &[u8], n: usize) -> Vec<u8> {
    bytes[..bytes.len() - n].to_vec()
}

#[test]
fn lookup_request() {
    let id1 = [0x11; 32];
    let id2 = [0x22; 32];
    let vector = head(b"OXLQ", 0b11)
        .u8(0b11)
        .u8(0)
        .u32(2)
        .raw(&id1)
        .raw(&id2)
        .done();
    let request = LookupRequest::decode(&vector).unwrap();
    assert_eq!(
        request.flags,
        LookupFlags {
            records: true,
            urls: true
        }
    );
    assert_eq!(
        request.namespaces,
        Namespaces {
            trusted: true,
            personal: true
        }
    );
    assert_eq!(request.ids, vec![LookupId(id1), LookupId(id2)]);
    assert_eq!(request.encode().unwrap(), vector);

    let plan = head(b"OXLQ", 0).u8(0b01).u8(0).u32(0).done();
    let request = LookupRequest::decode(&plan).unwrap();
    assert_eq!(
        request.flags,
        LookupFlags {
            records: false,
            urls: false
        }
    );
    assert_eq!(
        request.namespaces,
        Namespaces {
            trusted: true,
            personal: false
        }
    );
    assert!(request.ids.is_empty());

    let bad = |v: Vec<u8>| LookupRequest::decode(&v).unwrap_err();
    assert_eq!(bad(with(&vector, 3, b'X')), FormatError::BadMagic);
    assert_eq!(bad(with(&vector, 4, 2)), FormatError::UnsupportedVersion(2));
    assert_eq!(
        bad(with(&vector, 5, 0b100)),
        FormatError::UnknownFlags(0b100)
    );
    assert_eq!(
        bad(with(&vector, 6, 0b100)),
        FormatError::Invalid("namespaces")
    );
    assert_eq!(bad(with(&vector, 6, 0)), FormatError::Invalid("namespaces"));
    assert_eq!(bad(with(&vector, 7, 1)), FormatError::NonZeroReserved);
    assert_eq!(bad(minus(&vector, 1)), FormatError::Truncated);
    assert_eq!(bad(plus(&vector, &[0])), FormatError::TrailingBytes);
    assert_eq!(bad(vector[..5].to_vec()), FormatError::Truncated);

    let over = head(b"OXLQ", 0).u8(1).u8(0).u32(10_001).done();
    assert!(matches!(bad(over), FormatError::LimitExceeded(_)));
    let claims_limit = head(b"OXLQ", 0).u8(1).u8(0).u32(10_000).done();
    assert_eq!(bad(claims_limit), FormatError::Truncated);
    let at_limit = head(b"OXLQ", 0)
        .u8(1)
        .u8(0)
        .u32(10_000)
        .fill(7, 10_000 * 32)
        .done();
    assert_eq!(LookupRequest::decode(&at_limit).unwrap().ids.len(), 10_000);
}

fn lookup_shape(records: bool, urls: bool, count: u32) -> LookupShape {
    LookupShape {
        flags: LookupFlags { records, urls },
        namespaces: Namespaces {
            trusted: true,
            personal: true,
        },
        count,
    }
}

#[test]
fn lookup_response_fetch_mode() {
    let shape = lookup_shape(true, true, 3);
    let vector = head(b"OXLR", 0b11)
        .u16(0)
        .u32(1)
        .raw(&[0xaa; 16])
        .s16("https://store.example/p1")
        .u64(1_793_000_900)
        .u32(2)
        .u32(0)
        .u8(0)
        .u32(0)
        .u64(24)
        .u32(200)
        .u16(3)
        .raw(b"rec")
        .u32(2)
        .u8(1)
        .u32(0)
        .u64(224)
        .u32(150)
        .u16(4)
        .raw(b"recd")
        .u32(1)
        .raw(&[0x33; 32])
        .u32(0)
        .u64(374)
        .u32(120)
        .done();
    let response = LookupResponse::decode(&vector, &shape).unwrap();
    assert_eq!(response.flags, shape.flags);
    assert_eq!(
        response.packs,
        vec![ResponsePack {
            pack_id: PackId([0xaa; 16]),
            url: "https://store.example/p1",
            expires_at: 1_793_000_900
        }]
    );
    assert_eq!(
        response.hits,
        vec![
            LookupHit {
                index: 0,
                ns: Namespace::Trusted,
                record: Some(HitRecord {
                    pack_idx: 0,
                    offset: 24,
                    length: 200,
                    record: b"rec"
                })
            },
            LookupHit {
                index: 2,
                ns: Namespace::Personal,
                record: Some(HitRecord {
                    pack_idx: 0,
                    offset: 224,
                    length: 150,
                    record: b"recd"
                })
            },
        ]
    );
    assert_eq!(
        response.dictionaries,
        vec![ResponseDictionary {
            dict_id: SegmentId([0x33; 32]),
            pack_idx: 0,
            offset: 374,
            length: 120
        }]
    );
    assert_eq!(response.encode(&shape).unwrap(), vector);

    let bad = |v: Vec<u8>| LookupResponse::decode(&v, &shape).unwrap_err();
    assert_eq!(bad(with(&vector, 0, b'Z')), FormatError::BadMagic);
    assert_eq!(bad(with(&vector, 4, 0)), FormatError::UnsupportedVersion(0));
    assert_eq!(
        bad(with(&vector, 5, 0b100)),
        FormatError::UnknownFlags(0b100)
    );
    assert!(matches!(
        bad(with(&vector, 5, 0b01)),
        FormatError::Invalid(_)
    ));
    assert_eq!(bad(with(&vector, 7, 1)), FormatError::NonZeroReserved);
    assert_eq!(bad(minus(&vector, 1)), FormatError::Truncated);
    assert_eq!(bad(plus(&vector, &[0])), FormatError::TrailingBytes);
    assert!(matches!(
        LookupResponse::decode(&vector, &lookup_shape(true, true, 2)).unwrap_err(),
        FormatError::Invalid(_)
    ));
    let only_trusted = LookupShape {
        namespaces: Namespaces {
            trusted: true,
            personal: false,
        },
        ..shape
    };
    assert!(matches!(
        LookupResponse::decode(&vector, &only_trusted).unwrap_err(),
        FormatError::Invalid(_) | FormatError::LimitExceeded(_)
    ));
}

#[test]
fn lookup_response_rejects_inconsistent_references() {
    let shape = lookup_shape(true, true, 3);
    let build = |pack_idx: u32, second_ns: u8, second_index: u32, dict_pack: u32, dups: bool| {
        let dict2 = if dups { [0x33; 32] } else { [0x34; 32] };
        head(b"OXLR", 0b11)
            .u16(0)
            .u32(1)
            .raw(&[0xaa; 16])
            .s16("https://x")
            .u64(1)
            .u32(2)
            .u32(0)
            .u8(0)
            .u32(pack_idx)
            .u64(24)
            .u32(200)
            .u16(1)
            .raw(b"a")
            .u32(second_index)
            .u8(second_ns)
            .u32(0)
            .u64(24)
            .u32(200)
            .u16(1)
            .raw(b"b")
            .u32(2)
            .raw(&[0x33; 32])
            .u32(dict_pack)
            .u64(24)
            .u32(200)
            .raw(&dict2)
            .u32(0)
            .u64(24)
            .u32(200)
            .done()
    };
    assert!(LookupResponse::decode(&build(0, 1, 0, 0, false), &shape).is_ok());
    let bad = |v: Vec<u8>| LookupResponse::decode(&v, &shape).unwrap_err();
    assert_eq!(
        bad(build(1, 1, 0, 0, false)),
        FormatError::Invalid("pack index")
    );
    assert_eq!(
        bad(build(0, 1, 0, 1, false)),
        FormatError::Invalid("pack index")
    );
    assert_eq!(
        bad(build(0, 0, 0, 0, false)),
        FormatError::Invalid("duplicate hit")
    );
    assert_eq!(
        bad(build(0, 1, 3, 0, false)),
        FormatError::Invalid("hit outside the request")
    );
    assert_eq!(
        bad(build(0, 2, 0, 0, false)),
        FormatError::Invalid("namespace")
    );
    assert_eq!(
        bad(build(0, 1, 0, 0, true)),
        FormatError::Invalid("duplicate dictionary")
    );
}

#[test]
fn lookup_response_plan_mode() {
    let shape = lookup_shape(false, false, 2);
    let vector = head(b"OXLR", 0)
        .u16(0)
        .u32(1)
        .raw(&[0xaa; 16])
        .s16("")
        .u64(0)
        .u32(1)
        .u32(1)
        .u8(0)
        .u32(0)
        .done();
    let response = LookupResponse::decode(&vector, &shape).unwrap();
    assert_eq!(
        response.hits,
        vec![LookupHit {
            index: 1,
            ns: Namespace::Trusted,
            record: None
        }]
    );
    assert_eq!(response.packs[0].url, "");
    assert!(response.dictionaries.is_empty());
    assert_eq!(response.encode(&shape).unwrap(), vector);

    let with_url = head(b"OXLR", 0)
        .u16(0)
        .u32(1)
        .raw(&[0xaa; 16])
        .s16("https://x")
        .u64(0)
        .u32(0)
        .u32(0)
        .done();
    assert!(matches!(
        LookupResponse::decode(&with_url, &shape).unwrap_err(),
        FormatError::Invalid(_)
    ));
    let too_many_hits = head(b"OXLR", 0).u16(0).u32(0).u32(5).done();
    assert!(matches!(
        LookupResponse::decode(&too_many_hits, &shape).unwrap_err(),
        FormatError::LimitExceeded(_)
    ));
}

fn commit_vector() -> Vec<u8> {
    head(b"OXCM", 0)
        .u8(0)
        .u8(0)
        .u64(13_107_200)
        .u32(2)
        .raw(&[0x01; 32])
        .u64(24)
        .u32(5000)
        .u8(0)
        .raw(&[0x02; 32])
        .u64(5024)
        .u32(900)
        .u8(1)
        .u32(2)
        .raw(&[0x0a; 32])
        .u32(0)
        .raw(&[0; 32])
        .u32(3)
        .u16(4)
        .raw(b"rec1")
        .raw(&[0x0b; 32])
        .u32(0)
        .raw(&[0x02; 32])
        .u32(3)
        .u16(0)
        .done()
}

#[test]
fn commit_request() {
    let vector = commit_vector();
    let request = CommitRequest::decode(&vector).unwrap();
    assert_eq!(request.ns, Namespace::Trusted);
    assert_eq!(request.pack_size, 13_107_200);
    assert_eq!(
        request.segments,
        vec![
            CommitSegment {
                segment_id: SegmentId([1; 32]),
                offset: 24,
                length: 5000,
                kind: SegmentKind::Artifact
            },
            CommitSegment {
                segment_id: SegmentId([2; 32]),
                offset: 5024,
                length: 900,
                kind: SegmentKind::Dictionary
            },
        ]
    );
    assert_eq!(
        request.entries,
        vec![
            CommitEntry {
                lookup_id: LookupId([0x0a; 32]),
                segment_idx: 0,
                dict_id: None,
                epoch: 3,
                record: b"rec1"
            },
            CommitEntry {
                lookup_id: LookupId([0x0b; 32]),
                segment_idx: 0,
                dict_id: Some(SegmentId([2; 32])),
                epoch: 3,
                record: b""
            },
        ]
    );
    assert_eq!(request.encode().unwrap(), vector);

    let bad = |v: Vec<u8>| CommitRequest::decode(&v).unwrap_err();
    assert_eq!(bad(with(&vector, 1, b'Z')), FormatError::BadMagic);
    assert_eq!(bad(with(&vector, 4, 9)), FormatError::UnsupportedVersion(9));
    assert_eq!(bad(with(&vector, 5, 1)), FormatError::UnknownFlags(1));
    assert_eq!(bad(with(&vector, 6, 2)), FormatError::Invalid("namespace"));
    assert_eq!(bad(with(&vector, 7, 1)), FormatError::NonZeroReserved);
    assert_eq!(bad(minus(&vector, 1)), FormatError::Truncated);
    assert_eq!(bad(plus(&vector, &[0])), FormatError::TrailingBytes);
    assert!(matches!(
        bad(with(&vector, 8, 1)),
        FormatError::LimitExceeded(_)
    ));

    let personal_dictionary = with(&vector, 6, 1);
    assert_eq!(
        bad(personal_dictionary),
        FormatError::Invalid("dictionary segment in a personal commit")
    );
    let unknown_kind = with(&vector, 6 + 2 + 8 + 4 + 45 + 44, 2);
    assert_eq!(bad(unknown_kind), FormatError::Invalid("segment kind"));

    let segments = |n: u32| head(b"OXCM", 0).u8(0).u8(0).u64(100).u32(n).done();
    assert!(matches!(bad(segments(2001)), FormatError::LimitExceeded(_)));
    assert_eq!(bad(segments(2000)), FormatError::Truncated);
    let entries = |n: u32| head(b"OXCM", 0).u8(0).u8(0).u64(100).u32(0).u32(n).done();
    assert!(matches!(bad(entries(2001)), FormatError::LimitExceeded(_)));
    assert!(CommitRequest::decode(&entries(0)).is_ok());
    let big_pack = head(b"OXCM", 0)
        .u8(0)
        .u8(0)
        .u64(32 * 1024 * 1024 + 1)
        .u32(0)
        .u32(0)
        .done();
    assert!(matches!(bad(big_pack), FormatError::LimitExceeded(_)));
    let max_pack = head(b"OXCM", 0)
        .u8(0)
        .u8(0)
        .u64(32 * 1024 * 1024)
        .u32(0)
        .u32(0)
        .done();
    assert!(CommitRequest::decode(&max_pack).is_ok());

    let entry = |record_len: u16| {
        head(b"OXCM", 0)
            .u8(0)
            .u8(0)
            .u64(100)
            .u32(0)
            .u32(1)
            .fill(1, 32)
            .u32(0)
            .fill(0, 32)
            .u32(0)
            .u16(record_len)
            .fill(9, record_len as usize)
            .done()
    };
    assert!(CommitRequest::decode(&entry(4096)).is_ok());
    assert!(matches!(bad(entry(4097)), FormatError::LimitExceeded(_)));
}

#[test]
fn commit_response() {
    let vector = head(b"OXCA", 0)
        .u16(0)
        .u32(4)
        .u8(0)
        .u8(1)
        .u8(2)
        .u8(3)
        .done();
    let response = CommitResponse::decode(&vector).unwrap();
    assert_eq!(
        response.statuses,
        vec![
            CommitStatus::Committed,
            CommitStatus::Exists,
            CommitStatus::Invalid,
            CommitStatus::MissingDictionary
        ]
    );
    assert_eq!(response.encode().unwrap(), vector);

    let bad = |v: Vec<u8>| CommitResponse::decode(&v).unwrap_err();
    assert_eq!(bad(with(&vector, 2, b'Z')), FormatError::BadMagic);
    assert_eq!(bad(with(&vector, 4, 2)), FormatError::UnsupportedVersion(2));
    assert_eq!(bad(with(&vector, 5, 1)), FormatError::UnknownFlags(1));
    assert_eq!(bad(with(&vector, 7, 1)), FormatError::NonZeroReserved);
    assert_eq!(
        bad(with(&vector, 12, 4)),
        FormatError::Invalid("commit status")
    );
    assert_eq!(bad(minus(&vector, 1)), FormatError::Truncated);
    assert_eq!(bad(plus(&vector, &[0])), FormatError::TrailingBytes);
    let over = head(b"OXCA", 0).u16(0).u32(2001).done();
    assert!(matches!(bad(over), FormatError::LimitExceeded(_)));
}

struct PackVector {
    header: Vec<u8>,
    footer: Vec<u8>,
    trailer: Vec<u8>,
    object_size: u64,
}

fn pack_vector() -> PackVector {
    let header = head(b"OXPK", 0).u16(0).raw(&[0x55; 16]).done();
    let footer = B::new()
        .raw(&[0x01; 32])
        .u64(24)
        .u32(102)
        .raw(&[0x02; 32])
        .u64(126)
        .u32(150)
        .done();
    let digest = *blake3::hash(&footer).as_bytes();
    let trailer = B::new().u32(2).u64(276).raw(&digest).raw(b"OXPF").done();
    PackVector {
        header,
        footer,
        trailer,
        object_size: 276 + 88 + 48,
    }
}

#[test]
fn pack_object() {
    let p = pack_vector();
    assert_eq!(p.header.len(), 24);
    assert_eq!(p.trailer.len(), 48);

    assert_eq!(
        PackHeader::decode(&p.header).unwrap().cache_id,
        CacheId([0x55; 16])
    );
    assert_eq!(
        PackHeader::decode(&p.header).unwrap().encode().as_slice(),
        p.header
    );
    let bad_header = |v: Vec<u8>| PackHeader::decode(&v).unwrap_err();
    assert_eq!(bad_header(with(&p.header, 0, b'Q')), FormatError::BadMagic);
    assert_eq!(
        bad_header(with(&p.header, 4, 2)),
        FormatError::UnsupportedVersion(2)
    );
    assert_eq!(
        bad_header(with(&p.header, 5, 1)),
        FormatError::UnknownFlags(1)
    );
    assert_eq!(
        bad_header(with(&p.header, 7, 1)),
        FormatError::NonZeroReserved
    );
    assert_eq!(bad_header(minus(&p.header, 1)), FormatError::Truncated);
    assert_eq!(
        bad_header(plus(&p.header, &[0])),
        FormatError::TrailingBytes
    );

    let trailer = PackTrailer::decode(&p.trailer, p.object_size).unwrap();
    assert_eq!(trailer.segment_count, 2);
    assert_eq!(trailer.footer_offset, 276);
    assert_eq!(trailer.footer_digest, *blake3::hash(&p.footer).as_bytes());
    assert_eq!(trailer.encode().unwrap().as_slice(), p.trailer);
    let bad_trailer = |v: Vec<u8>, size: u64| PackTrailer::decode(&v, size).unwrap_err();
    assert_eq!(
        bad_trailer(with(&p.trailer, 47, b'G'), p.object_size),
        FormatError::BadMagic
    );
    assert!(matches!(
        bad_trailer(p.trailer.clone(), p.object_size + 1),
        FormatError::Invalid(_)
    ));
    assert_eq!(
        bad_trailer(minus(&p.trailer, 1), p.object_size),
        FormatError::Truncated
    );
    assert_eq!(
        bad_trailer(plus(&p.trailer, &[0]), p.object_size),
        FormatError::TrailingBytes
    );
    let many = B::new().u32(2001).u64(24).fill(0, 32).raw(b"OXPF").done();
    assert!(matches!(
        bad_trailer(many, 24 + 2001 * 44 + 48),
        FormatError::LimitExceeded(_)
    ));
    let early_footer = B::new().u32(0).u64(23).fill(0, 32).raw(b"OXPF").done();
    assert!(matches!(
        bad_trailer(early_footer, 23 + 48),
        FormatError::Invalid(_)
    ));
    let oversize = B::new()
        .u32(0)
        .u64(32 * 1024 * 1024)
        .fill(0, 32)
        .raw(b"OXPF")
        .done();
    assert!(matches!(
        bad_trailer(oversize, 32 * 1024 * 1024 + 48),
        FormatError::LimitExceeded(_)
    ));

    let footer = PackFooter::decode(&p.footer, &trailer, |b| *blake3::hash(b).as_bytes()).unwrap();
    assert_eq!(
        footer.segments,
        vec![
            PackSegment {
                segment_id: SegmentId([1; 32]),
                offset: 24,
                length: 102
            },
            PackSegment {
                segment_id: SegmentId([2; 32]),
                offset: 126,
                length: 150
            },
        ]
    );
    assert_eq!(footer.encode().unwrap(), p.footer);
    let bad_footer =
        |v: Vec<u8>| PackFooter::decode(&v, &trailer, |b| *blake3::hash(b).as_bytes()).unwrap_err();
    assert_eq!(
        bad_footer(with(&p.footer, 0, 9)),
        FormatError::Invalid("footer digest")
    );
    assert!(matches!(
        bad_footer(minus(&p.footer, 1)),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad_footer(plus(&p.footer, &[0])),
        FormatError::Invalid(_)
    ));

    let gap_footer = B::new()
        .raw(&[0x01; 32])
        .u64(24)
        .u32(102)
        .raw(&[0x02; 32])
        .u64(127)
        .u32(150)
        .done();
    let gap_trailer = PackTrailer {
        segment_count: 2,
        footer_offset: 276,
        footer_digest: *blake3::hash(&gap_footer).as_bytes(),
    };
    assert_eq!(
        PackFooter::decode(&gap_footer, &gap_trailer, |b| *blake3::hash(b).as_bytes()).unwrap_err(),
        FormatError::Invalid("segment layout")
    );
    let wrong_offset = PackTrailer {
        footer_offset: 277,
        ..trailer
    };
    assert!(PackFooter::decode(&p.footer, &wrong_offset, |b| *blake3::hash(b).as_bytes()).is_err());
}

#[test]
fn sealed_segment() {
    let header = B::new()
        .raw(b"OXSG")
        .u8(1)
        .u8(1)
        .u8(16)
        .u8(0)
        .raw(&[0x01; 32])
        .raw(&[0x02; 32])
        .done();
    assert_eq!(header.len(), 72);
    let decoded = SegmentHeader::decode(&header).unwrap();
    assert_eq!(
        decoded,
        SegmentHeader {
            salt: [1; 32],
            k_commit: [2; 32]
        }
    );
    assert_eq!(decoded.encode().as_slice(), header);

    let bad_header = |v: Vec<u8>| SegmentHeader::decode(&v).unwrap_err();
    assert_eq!(bad_header(with(&header, 0, b'Z')), FormatError::BadMagic);
    assert_eq!(
        bad_header(with(&header, 4, 2)),
        FormatError::UnsupportedVersion(2)
    );
    assert_eq!(
        bad_header(with(&header, 5, 2)),
        FormatError::Invalid("segment algorithm")
    );
    assert_eq!(
        bad_header(with(&header, 6, 15)),
        FormatError::Invalid("chunk size")
    );
    assert_eq!(
        bad_header(with(&header, 7, 1)),
        FormatError::NonZeroReserved
    );
    assert_eq!(bad_header(minus(&header, 1)), FormatError::Truncated);
    assert_eq!(bad_header(plus(&header, &[0])), FormatError::TrailingBytes);

    let segment = |body: usize| plus(&header, &vec![0xee; body]);
    for body in [30, 17, 65_552, 65_552 + 17, 2 * 65_552 + 30] {
        let bytes = segment(body);
        let s = SealedSegment::decode(&bytes).unwrap();
        assert_eq!(s.header, decoded);
        assert_eq!(s.body.len(), body);
        assert_eq!(s.encode().unwrap(), bytes);
    }
    for body in [0, 1, 16, 65_552 + 16, 2 * 65_552 + 1] {
        assert_eq!(
            SealedSegment::decode(&segment(body)).unwrap_err(),
            FormatError::Invalid("chunk structure"),
            "body of {body} bytes"
        );
    }
}

#[test]
fn segment_plaintext() {
    let vector = head(b"OXSP", 1).u64(5).raw(b"hello").u8(0).done();
    assert_eq!(vector.len(), 20);
    assert_eq!(padme(19), Some(20));
    let plain = SegmentPlaintext::decode(&vector).unwrap();
    assert_eq!(
        plain,
        SegmentPlaintext {
            codec: Codec::Zstd,
            compressed: b"hello"
        }
    );
    assert_eq!(plain.encode().unwrap(), vector);

    let empty = head(b"OXSP", 0).u64(0).done();
    assert_eq!(empty.len(), 14);
    assert_eq!(SegmentPlaintext::decode(&empty).unwrap().codec, Codec::None);
    let dict = head(b"OXSP", 2).u64(5).raw(b"hello").u8(0).done();
    assert_eq!(
        SegmentPlaintext::decode(&dict).unwrap().codec,
        Codec::ZstdDictionary
    );

    let bad = |v: Vec<u8>| SegmentPlaintext::decode(&v).unwrap_err();
    assert_eq!(bad(with(&vector, 0, b'Z')), FormatError::BadMagic);
    assert_eq!(bad(with(&vector, 4, 2)), FormatError::UnsupportedVersion(2));
    assert_eq!(bad(with(&vector, 5, 3)), FormatError::Invalid("codec"));
    assert_eq!(
        bad(with(&vector, 19, 1)),
        FormatError::Invalid("padding is not zero")
    );
    assert_eq!(
        bad(plus(&vector, &[0])),
        FormatError::Invalid("padding length")
    );
    assert_eq!(
        bad(minus(&vector, 1)),
        FormatError::Invalid("padding length")
    );
    assert_eq!(bad(minus(&vector, 2)), FormatError::Truncated);
    let overlong = head(b"OXSP", 1).u64(100).raw(b"hello").done();
    assert_eq!(bad(overlong), FormatError::Truncated);
    let huge = head(b"OXSP", 1).u64(u64::MAX).raw(b"hello").done();
    assert_eq!(bad(huge), FormatError::Truncated);
    assert_eq!(bad(vector[..9].to_vec()), FormatError::Truncated);
}

#[test]
fn padme_lengths() {
    for (len, padded) in [
        (0, 0),
        (1, 1),
        (2, 2),
        (14, 14),
        (19, 20),
        (1000, 1024),
        (4096, 4096),
        (100_001, 100_352),
    ] {
        assert_eq!(padme(len), Some(padded), "padme({len})");
    }
}

fn record_header() -> B {
    B::new()
        .raw(b"OXER")
        .u8(1)
        .u8(1)
        .u16(0)
        .raw(&[0x01; 32])
        .raw(&[0x02; 32])
}

#[test]
fn sealed_record() {
    let vector = record_header().fill(0xcd, 512 - 72).done();
    let record = SealedRecord::decode(&vector).unwrap();
    assert_eq!(record.salt, [1; 32]);
    assert_eq!(record.k_commit, [2; 32]);
    assert_eq!(record.ciphertext, &[0xcd; 440][..]);
    assert_eq!(record.encode().unwrap(), vector);
    assert!(SealedRecord::decode(&record_header().fill(0, 4096 - 72).done()).is_ok());

    let bad = |v: Vec<u8>| SealedRecord::decode(&v).unwrap_err();
    assert_eq!(bad(with(&vector, 0, b'Z')), FormatError::BadMagic);
    assert_eq!(bad(with(&vector, 4, 2)), FormatError::UnsupportedVersion(2));
    assert_eq!(
        bad(with(&vector, 5, 2)),
        FormatError::Invalid("record algorithm")
    );
    assert_eq!(bad(with(&vector, 7, 1)), FormatError::NonZeroReserved);
    assert_eq!(bad(vector[..71].to_vec()), FormatError::Truncated);
    assert_eq!(
        bad(minus(&vector, 1)),
        FormatError::Invalid("sealed record length")
    );
    assert_eq!(
        bad(plus(&vector, &[0])),
        FormatError::Invalid("sealed record length")
    );
    assert_eq!(
        bad(record_header().fill(0, 256 - 72).done()),
        FormatError::Invalid("sealed record length")
    );
    assert!(matches!(
        bad(record_header().fill(0, 4352 - 72).done()),
        FormatError::LimitExceeded(_)
    ));
}

fn entry_plaintext(files: &[(&str, u64)], plain_len: u64, padding: usize) -> Vec<u8> {
    let mut b = head(b"OXEP", 0)
        .raw(&[0xaa; 32])
        .u32(4096)
        .fill(0, 32)
        .u64(plain_len)
        .i64(1_700_000_000)
        .u64(7)
        .raw(&[0xbb; 32])
        .u16(files.len() as u16);
    for (path, size) in files {
        b = b.s16(path).u64(*size);
    }
    b.fill(0xcc, 64).fill(0, padding).done()
}

fn entry_padding(files: &[(&str, u64)]) -> usize {
    let unpadded = 132 + 64 + files.iter().map(|(p, _)| 2 + p.len() + 8).sum::<usize>();
    (256 - (72 + 16 + unpadded) % 256) % 256
}

fn entry_for(files: &[(&str, u64)]) -> Vec<u8> {
    entry_plaintext(
        files,
        files.iter().map(|(_, s)| s).sum(),
        entry_padding(files),
    )
}

#[test]
fn entry_payload() {
    let limits = Limits::default();
    let files = [("ar/x.r", 100), ("ar/y.r", 200)];
    let vector = entry_for(&files);
    assert_eq!(vector.len(), 424);
    let payload = EntryPayload::decode(&vector, &limits).unwrap();
    assert_eq!(payload.segment_id, SegmentId([0xaa; 32]));
    assert_eq!(payload.segment_len, 4096);
    assert_eq!(payload.dict_id, None);
    assert_eq!(payload.plain_len, 300);
    assert_eq!(payload.created_at, 1_700_000_000);
    assert_eq!(payload.roster_version, 7);
    assert_eq!(payload.signer, [0xbb; 32]);
    assert_eq!(
        payload.files,
        vec![
            EntryFile {
                path: "ar/x.r",
                size: 100
            },
            EntryFile {
                path: "ar/y.r",
                size: 200
            }
        ]
    );
    assert_eq!(payload.signature, [0xcc; 64]);
    assert_eq!(payload.encode(&limits).unwrap(), vector);
    assert_eq!(payload.signed_bytes(&limits).unwrap(), vector[..132 + 32]);

    let bad = |v: Vec<u8>| EntryPayload::decode(&v, &limits).unwrap_err();
    assert_eq!(bad(with(&vector, 0, b'Z')), FormatError::BadMagic);
    assert_eq!(bad(with(&vector, 4, 2)), FormatError::UnsupportedVersion(2));
    assert_eq!(bad(with(&vector, 5, 1)), FormatError::UnknownFlags(1));
    assert_eq!(
        bad(with(&vector, 423, 1)),
        FormatError::Invalid("padding is not zero")
    );
    assert_eq!(
        bad(minus(&vector, 1)),
        FormatError::Invalid("padding length")
    );
    assert_eq!(
        bad(plus(&vector, &[0])),
        FormatError::Invalid("padding length")
    );
    assert_eq!(bad(vector[..100].to_vec()), FormatError::Truncated);
    assert_eq!(
        bad(entry_plaintext(&files, 301, entry_padding(&files))),
        FormatError::Invalid("plain_len differs from the file sizes")
    );
    let tiny_segment = {
        let mut v = vector.clone();
        v[38..42].copy_from_slice(&101u32.to_be_bytes());
        v
    };
    assert_eq!(bad(tiny_segment), FormatError::Invalid("segment_len"));

    let dict = {
        let mut v = vector.clone();
        v[42..74].copy_from_slice(&[0x77; 32]);
        v
    };
    assert_eq!(
        EntryPayload::decode(&dict, &limits).unwrap().dict_id,
        Some(SegmentId([0x77; 32]))
    );

    let names: Vec<String> = (0..16).map(|i| format!("d/f{i}.r")).collect();
    let sixteen: Vec<(&str, u64)> = names.iter().map(|n| (n.as_str(), 1)).collect();
    assert_eq!(
        EntryPayload::decode(&entry_for(&sixteen), &limits)
            .unwrap()
            .files
            .len(),
        16
    );
    let mut seventeen = sixteen.clone();
    seventeen.push(("d/extra.r", 1));
    assert!(matches!(
        bad(entry_for(&seventeen)),
        FormatError::LimitExceeded(_)
    ));

    assert_eq!(
        bad(entry_for(&[("a.r", 1), ("a.r", 1)])),
        FormatError::Invalid("duplicate path")
    );
    for path in ["", "/abs/x.r", "ar/../x.r", "..", "a\0b"] {
        assert_eq!(
            bad(entry_for(&[(path, 1)])),
            FormatError::Invalid("path"),
            "path {path:?}"
        );
    }
    let long = "a".repeat(1025);
    assert!(matches!(
        bad(entry_for(&[(long.as_str(), 1)])),
        FormatError::LimitExceeded(_)
    ));
    let max_path = "a".repeat(1024);
    assert!(EntryPayload::decode(&entry_for(&[(max_path.as_str(), 1)]), &limits).is_ok());
    let k = "k".repeat(1000);
    let l = "l".repeat(1000);
    let m = "m".repeat(1000);
    let n = "n".repeat(100);
    let over_set = [
        (k.as_str(), 1),
        (l.as_str(), 1),
        (m.as_str(), 1),
        (n.as_str(), 1),
    ];
    assert!(matches!(
        bad(entry_for(&over_set)),
        FormatError::LimitExceeded(_)
    ));

    let big = [("big.r", 64 * 1024 * 1024 + 1)];
    assert!(matches!(
        bad(entry_for(&big)),
        FormatError::LimitExceeded(_)
    ));
    let raised = Limits::new(1024 * 1024 * 1024).unwrap();
    assert!(EntryPayload::decode(&entry_for(&big), &raised).is_ok());
    assert!(Limits::new(1024 * 1024 * 1024 + 1).is_err());
    let exact = [("big.r", 64 * 1024 * 1024)];
    assert!(EntryPayload::decode(&entry_for(&exact), &limits).is_ok());
}

fn roster_vector() -> Vec<u8> {
    head(b"OXRS", 0)
        .raw(&[0x10; 16])
        .u64(3)
        .u32(2)
        .u16(2)
        .s16("ci-runner")
        .u8(1)
        .u8(1)
        .raw(&[0x21; 32])
        .u8(0)
        .s16("alice")
        .u8(0)
        .u8(2)
        .raw(&[0x22; 32])
        .raw(&[0x23; 32])
        .u8(1)
        .s16("age1abc")
        .u32(5)
        .raw(b"AGEKR")
        .u8(1)
        .raw(&[0x30; 32])
        .raw(&[0x40; 64])
        .done()
}

#[test]
fn roster() {
    let vector = roster_vector();
    let roster = Roster::decode(&vector).unwrap();
    assert_eq!(roster.cache_id, CacheId([0x10; 16]));
    assert_eq!(roster.version, 3);
    assert_eq!(roster.epoch, 2);
    assert_eq!(
        roster.members,
        vec![
            Member {
                label: "ci-runner",
                role: Role::Ci,
                sign_keys: vec![[0x21; 32]],
                age_recipients: vec![]
            },
            Member {
                label: "alice",
                role: Role::Owner,
                sign_keys: vec![[0x22; 32], [0x23; 32]],
                age_recipients: vec!["age1abc"]
            },
        ]
    );
    assert_eq!(roster.keyring, b"AGEKR");
    assert_eq!(
        roster.signatures,
        vec![RosterSignature {
            owner_key: [0x30; 32],
            signature: [0x40; 64]
        }]
    );
    assert_eq!(roster.encode().unwrap(), vector);
    assert_eq!(
        roster.signed_bytes().unwrap(),
        vector[..vector.len() - 1 - 96]
    );

    let bad = |v: Vec<u8>| Roster::decode(&v).unwrap_err();
    assert_eq!(bad(with(&vector, 0, b'Z')), FormatError::BadMagic);
    assert_eq!(bad(with(&vector, 4, 2)), FormatError::UnsupportedVersion(2));
    assert_eq!(bad(with(&vector, 5, 1)), FormatError::UnknownFlags(1));
    assert_eq!(bad(minus(&vector, 1)), FormatError::Truncated);
    assert_eq!(bad(plus(&vector, &[0])), FormatError::TrailingBytes);
    let role_at = 6 + 16 + 8 + 4 + 2 + 2 + 9;
    assert_eq!(bad(with(&vector, role_at, 4)), FormatError::Invalid("role"));
    let utf8_at = role_at - 1;
    assert_eq!(bad(with(&vector, utf8_at, 0xff)), FormatError::InvalidUtf8);
    let sig_count_at = vector.len() - 96 - 1;
    assert_eq!(
        bad(with(&vector, sig_count_at, 0)),
        FormatError::Invalid("signature count")
    );
    assert_eq!(
        bad(with(&vector, sig_count_at, 5)),
        FormatError::Invalid("signature count")
    );

    let two_sigs = {
        let mut v = with(&vector, sig_count_at, 2);
        v.extend_from_slice(&[0x31; 32]);
        v.extend_from_slice(&[0x41; 64]);
        v
    };
    assert_eq!(Roster::decode(&two_sigs).unwrap().signatures.len(), 2);

    let members = |n: u16| head(b"OXRS", 0).raw(&[0; 16]).u64(1).u32(1).u16(n).done();
    assert!(matches!(bad(members(1001)), FormatError::LimitExceeded(_)));
    assert_eq!(bad(members(1000)), FormatError::Truncated);
    let overrun = head(b"OXRS", 0)
        .raw(&[0; 16])
        .u64(1)
        .u32(1)
        .u16(0)
        .u32(100)
        .raw(b"short")
        .done();
    assert_eq!(bad(overrun), FormatError::Truncated);
    let huge = head(b"OXRS", 0)
        .raw(&[0; 16])
        .u64(1)
        .u32(1)
        .u16(0)
        .u32(1024 * 1024)
        .fill(0, 1024 * 1024)
        .u8(1)
        .fill(0, 96)
        .done();
    assert!(matches!(bad(huge), FormatError::LimitExceeded(_)));
}

#[test]
fn dictionary_pointer() {
    let vector = head(b"OXDP", 0)
        .raw(&[0x10; 16])
        .u32(4)
        .u64(9)
        .raw(&[0x50; 32])
        .raw(&[0x60; 32])
        .raw(&[0x70; 64])
        .done();
    assert_eq!(vector.len(), 162);
    let pointer = DictionaryPointer::decode(&vector).unwrap();
    assert_eq!(pointer.cache_id, CacheId([0x10; 16]));
    assert_eq!(pointer.epoch, 4);
    assert_eq!(pointer.version, 9);
    assert_eq!(pointer.dict_segment_id, SegmentId([0x50; 32]));
    assert_eq!(pointer.signer, [0x60; 32]);
    assert_eq!(pointer.signature, [0x70; 64]);
    assert_eq!(pointer.encode(), vector);
    assert_eq!(pointer.signed_bytes(), vector[..98]);

    let bad = |v: Vec<u8>| DictionaryPointer::decode(&v).unwrap_err();
    assert_eq!(bad(with(&vector, 0, b'Z')), FormatError::BadMagic);
    assert_eq!(bad(with(&vector, 4, 2)), FormatError::UnsupportedVersion(2));
    assert_eq!(bad(with(&vector, 5, 1)), FormatError::UnknownFlags(1));
    assert_eq!(bad(minus(&vector, 1)), FormatError::Truncated);
    assert_eq!(bad(plus(&vector, &[0])), FormatError::TrailingBytes);
}

#[test]
fn identity_strings() {
    let descriptor = MemberDescriptor {
        ed25519_public: [1; 32],
        x25519_public: [2; 32],
    };
    let text = "oxabl-cache-member-1qyqszqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgpqyqszqszqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgpqyqszqgpqyqszpnwdql";
    assert_eq!(MemberDescriptor::decode(text).unwrap(), descriptor);
    assert_eq!(descriptor.encode().unwrap(), text);

    let bad = |s: &str| MemberDescriptor::decode(s).unwrap_err();
    assert!(matches!(bad(&text.to_uppercase()), FormatError::Invalid(_)));
    let mut flipped = text.to_string();
    flipped.pop();
    flipped.push('q');
    assert_eq!(bad(&flipped), FormatError::Invalid("bech32 checksum"));
    assert!(matches!(
        bad(&text.replace("member", "membex")),
        FormatError::Invalid(_)
    ));
    assert!(matches!(bad(""), FormatError::Invalid(_)));
    assert!(matches!(
        bad(&text[..text.len() - 1]),
        FormatError::Invalid(_)
    ));
    assert!(matches!(bad(&format!("{text}q")), FormatError::Invalid(_)));

    let data = hex_to_bytes("00443214c74254b635cf84653a56d7c675be77df");
    let known = "abcdef1qpzry9x8gf2tvdw0s3jn54khce6mua7lmqqqxw";
    assert_eq!(bech32::decode(known, "abcdef").unwrap(), data);
    assert_eq!(bech32::encode("abcdef", &data).unwrap(), known);
    assert!(bech32::decode(known, "oxabl-cache-identity-").is_err());
    assert_eq!(bech32::IDENTITY_HRP, "oxabl-cache-identity-");
    let long = vec![0x5a; 65];
    let text = bech32::encode(bech32::IDENTITY_HRP, &long).unwrap();
    assert!(text.len() > 90);
    assert_eq!(bech32::decode(&text, bech32::IDENTITY_HRP).unwrap(), long);
}

fn hex_to_bytes(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

#[test]
fn text_ids() {
    assert_eq!(CacheId([0; 16]).to_text(), "c_00000000000000000000000000");
    assert_eq!(
        CacheId([0xff; 16]).to_text(),
        "c_7zzzzzzzzzzzzzzzzzzzzzzzzz"
    );
    let bytes: [u8; 16] = std::array::from_fn(|i| i as u8);
    assert_eq!(TenantId(bytes).to_text(), "t_00041061050r3gg28a1c60t3gf");
    assert_eq!(RequestId(bytes).to_text(), "00041061050r3gg28a1c60t3gf");
    assert_eq!(
        TenantId::from_text("t_00041061050r3gg28a1c60t3gf").unwrap(),
        TenantId(bytes)
    );
    assert_eq!(
        RequestId::from_text("00041061050r3gg28a1c60t3gf").unwrap(),
        RequestId(bytes)
    );
    assert_eq!(
        CacheId::from_text("c_7zzzzzzzzzzzzzzzzzzzzzzzzz").unwrap(),
        CacheId([0xff; 16])
    );

    let bad = |s: &str| CacheId::from_text(s).unwrap_err();
    assert!(matches!(
        bad("c_8zzzzzzzzzzzzzzzzzzzzzzzzz"),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad("c_7ZZZZZZZZZZZZZZZZZZZZZZZZZ"),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad("c_7zzzzzzzzzzzzzzzzzzzzzzzz"),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad("c_7zzzzzzzzzzzzzzzzzzzzzzzzzz"),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad("c_0000000000000000000000000i"),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad("c_0000000000000000000000000l"),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad("c_0000000000000000000000000o"),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad("c_0000000000000000000000000u"),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad("t_00000000000000000000000000"),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad("00000000000000000000000000"),
        FormatError::Invalid(_)
    ));

    let token = format!("oxc_{}_1{}", "z".repeat(16), "z".repeat(51));
    let parsed = ApiToken::from_text(&token).unwrap();
    assert_eq!(parsed.id, [0xff; 10]);
    assert_eq!(parsed.secret, [0xff; 32]);
    assert_eq!(parsed.to_text(), token);
    assert!(!format!("{parsed:?}").contains(&"z".repeat(51)));
    let bad_token = |s: &str| ApiToken::from_text(s).unwrap_err();
    assert!(matches!(
        bad_token(&token.replace("_1z", "_2z")),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad_token(&token.replace("oxc_", "oxs_")),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad_token(&token[..token.len() - 1]),
        FormatError::Invalid(_)
    ));
    assert!(matches!(
        bad_token(&token.replace("_1z", "1z")),
        FormatError::Invalid(_)
    ));

    let hex = PackId([0xab; 16]).to_hex();
    assert_eq!(hex, "ab".repeat(16));
    assert_eq!(PackId::from_hex(&hex).unwrap(), PackId([0xab; 16]));
    assert!(PackId::from_hex(&"AB".repeat(16)).is_err());
    assert!(PackId::from_hex(&"ab".repeat(15)).is_err());
    assert_eq!(
        SegmentId::from_hex(&"0f".repeat(32)).unwrap(),
        SegmentId([0x0f; 32])
    );
}

#[test]
fn published_limits() {
    assert_eq!(limits::LOOKUP_IDS, 10_000);
    assert_eq!(limits::SAMPLE_IDS, 1_000);
    assert_eq!(limits::COMMIT_ENTRIES, 2_000);
    assert_eq!(limits::PACK_SEGMENTS, 2_000);
    assert_eq!(limits::PACK_BYTES, 32 * 1024 * 1024);
    assert_eq!(limits::FILES_PER_SET, 16);
    assert_eq!(limits::PATH_BYTES, 1_024);
    assert_eq!(limits::SET_PATH_BYTES, 3_072);
    assert_eq!(limits::SEALED_RECORD_BYTES, 4_096);
    assert_eq!(limits::ROSTER_BYTES, 1024 * 1024);
    assert_eq!(limits::ROSTER_MEMBERS, 1_000);
    assert_eq!(Limits::default().plaintext_bytes(), 64 * 1024 * 1024);
}
