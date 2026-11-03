use std::io::Read;

use oxabl_cache_crypto::{
    Error, RootKey, SealOptions, SegmentExpectation, SegmentSeal, open_segment, seal_segment,
    sealed_segment_len,
};
use oxabl_cache_format::Limits;

use crate::support::data::{Rng, rcode_like};
use crate::support::reference;

pub fn root() -> RootKey {
    RootKey::from_bytes([9; 32])
}

pub fn seal(root: &RootKey, plain: &[u8]) -> (Vec<u8>, SegmentSeal) {
    let mut sealed = Vec::new();
    let info = seal_segment(
        &root.segment_key(),
        plain,
        &SealOptions::default(),
        &Limits::default(),
        &mut sealed,
    )
    .expect("seal");
    (sealed, info)
}

pub fn open(root: &RootKey, sealed: &[u8], info: &SegmentSeal) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    open_segment(
        &root.segment_key(),
        sealed,
        &SegmentExpectation {
            id: info.id,
            sealed_len: sealed.len() as u64,
            plain_len: info.plain_len,
            dictionary: None,
        },
        &Limits::default(),
        &mut out,
    )?;
    Ok(out)
}

#[test]
fn artifact_sets_round_trip_at_every_awkward_size() {
    let root = root();
    let mut rng = Rng::new(1);
    for len in [
        0usize, 1, 2, 13, 64, 4_000, 65_522, 65_523, 65_536, 65_537, 131_072, 300_000, 2_000_000,
    ] {
        for plain in [rng.bytes(len), rcode_like(&mut rng, len)] {
            let (sealed, info) = seal(&root, &plain);
            assert_eq!(info.plain_len, len as u64);
            assert_eq!(info.sealed_len, sealed.len() as u64);
            assert_eq!(
                open(&root, &sealed, &info).expect("open"),
                plain,
                "len {len}"
            );
        }
    }
}

#[test]
fn segment_id_is_the_blake3_of_the_sealed_bytes() {
    let root = root();
    let (sealed, info) = seal(&root, &rcode_like(&mut Rng::new(2), 100_000));
    assert_eq!(info.id.0, *blake3::hash(&sealed).as_bytes());
}

#[test]
fn sealed_length_is_predictable_from_the_compressed_length() {
    let root = root();
    let mut rng = Rng::new(3);
    for len in [10usize, 5_000, 70_000, 400_000] {
        let (sealed, info) = seal(&root, &rcode_like(&mut rng, len));
        assert_eq!(
            sealed_segment_len(info.compressed_len).expect("len"),
            sealed.len() as u64
        );
    }
}

#[test]
fn product_and_independent_reference_agree_on_the_wire_format() {
    let root = root();
    let mut rng = Rng::new(4);
    for len in [1usize, 90_000, 500_000] {
        let plain = rcode_like(&mut rng, len);
        let (sealed, info) = seal(&root, &plain);

        let plaintext = reference::open_segment_plaintext(&[9; 32], &sealed)
            .expect("reference opens product output");
        assert_eq!(&plaintext[..4], b"OXSP");
        assert_eq!(plaintext[5], 1);
        let compressed_len = u64::from_be_bytes(plaintext[6..14].try_into().expect("len")) as usize;
        assert_eq!(compressed_len as u64, info.compressed_len);
        assert_eq!(plaintext.len(), reference::padme(14 + compressed_len));
        assert!(plaintext[14 + compressed_len..].iter().all(|&b| b == 0));
        let frame = &plaintext[14..14 + compressed_len];
        assert_ne!(frame[4] & 0x04, 0, "zstd content checksum is on");
        assert_eq!(zstd::decode_all(frame).expect("zstd"), plain);

        let mut checksummed = Vec::new();
        let mut enc = zstd::stream::write::Encoder::new(&mut checksummed, 3).expect("encoder");
        enc.include_checksum(true).expect("checksum");
        std::io::Write::write_all(&mut enc, &plain).expect("write");
        enc.finish().expect("finish");
        let crafted = reference::seal_segment_plaintext(
            &[9; 32],
            &[5; 32],
            &reference::oxsp(1, &checksummed),
        );
        let crafted_info = SegmentSeal {
            id: oxabl_cache_format::SegmentId(*blake3::hash(&crafted).as_bytes()),
            sealed_len: crafted.len() as u64,
            plain_len: len as u64,
            compressed_len: checksummed.len() as u64,
        };
        assert_eq!(
            open(&root, &crafted, &crafted_info).expect("product opens reference output"),
            plain
        );
    }
}

struct Trickle<'a>(&'a [u8], usize);

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.0.len()).min(1 + self.1 % 7);
        self.1 += 1;
        buf[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

#[test]
fn input_arrives_through_a_reader_of_any_shape() {
    let root = root();
    let plain = rcode_like(&mut Rng::new(5), 50_000);
    let mut sealed = Vec::new();
    let info = seal_segment(
        &root.segment_key(),
        Trickle(&plain, 0),
        &SealOptions::default(),
        &Limits::default(),
        &mut sealed,
    )
    .expect("seal");
    assert_eq!(open(&root, &sealed, &info).expect("open"), plain);
}

#[test]
fn each_seal_uses_a_fresh_salt() {
    let root = root();
    let plain = rcode_like(&mut Rng::new(6), 1_000);
    let (a, _) = seal(&root, &plain);
    let (b, _) = seal(&root, &plain);
    assert_ne!(a[8..40], b[8..40]);
    assert_ne!(a, b);
}

#[test]
fn a_set_over_the_plaintext_limit_is_refused_while_sealing_and_opening() {
    let root = root();
    let limits = Limits::new(1_000).expect("limits");
    let plain = vec![7u8; 1_001];
    let mut sealed = Vec::new();
    assert!(matches!(
        seal_segment(
            &root.segment_key(),
            &plain[..],
            &SealOptions::default(),
            &limits,
            &mut sealed
        ),
        Err(Error::Limit(_))
    ));

    let (sealed, info) = seal(&root, &plain);
    let mut out = Vec::new();
    let result = open_segment(
        &root.segment_key(),
        &sealed,
        &SegmentExpectation {
            id: info.id,
            sealed_len: sealed.len() as u64,
            plain_len: info.plain_len,
            dictionary: None,
        },
        &limits,
        &mut out,
    );
    assert!(matches!(result, Err(Error::Limit(_))));
    assert!(out.is_empty());
}

#[test]
fn a_segment_opens_only_under_its_own_epoch_key() {
    let (sealed, info) = seal(&root(), &rcode_like(&mut Rng::new(7), 10_000));
    assert_eq!(
        open(&RootKey::from_bytes([10; 32]), &sealed, &info),
        Err(Error::KeyCommitment)
    );
}

#[test]
fn an_expectation_that_disagrees_with_the_bytes_is_refused() {
    let root = root();
    let (sealed, info) = seal(&root, &rcode_like(&mut Rng::new(8), 10_000));
    let expect = |id, sealed_len, plain_len| SegmentExpectation {
        id,
        sealed_len,
        plain_len,
        dictionary: None,
    };
    let try_open = |e: SegmentExpectation<'_>| {
        open_segment(
            &root.segment_key(),
            &sealed,
            &e,
            &Limits::default(),
            Vec::new(),
        )
    };
    assert!(try_open(expect(info.id, info.sealed_len, info.plain_len)).is_ok());
    assert!(matches!(
        try_open(expect(info.id, info.sealed_len + 1, info.plain_len)),
        Err(Error::Mismatch(_))
    ));
    assert!(matches!(
        try_open(expect(
            oxabl_cache_format::SegmentId([0; 32]),
            info.sealed_len,
            info.plain_len
        )),
        Err(Error::SegmentId)
    ));
    assert!(matches!(
        try_open(expect(info.id, info.sealed_len, info.plain_len - 1)),
        Err(Error::Compression(_))
    ));
    assert!(matches!(
        try_open(expect(info.id, info.sealed_len, info.plain_len + 1)),
        Err(Error::Compression(_))
    ));
}

#[test]
fn a_set_that_cannot_fit_a_pack_is_refused() {
    let root = root();
    let plain = Rng::new(9).bytes(36 * 1024 * 1024);
    let mut sealed = Vec::new();
    let result = seal_segment(
        &root.segment_key(),
        &plain[..],
        &SealOptions::default(),
        &Limits::default(),
        &mut sealed,
    );
    assert!(matches!(result, Err(Error::Limit("segment size"))));
    assert!(sealed.is_empty());
}
