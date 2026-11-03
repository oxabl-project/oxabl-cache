use oxabl_cache_crypto::{Error, RootKey, SegmentExpectation, SegmentSeal, open_segment};
use oxabl_cache_format::{Limits, SegmentId};

use crate::segments::{open, root, seal};
use crate::support::data::{Rng, rcode_like};
use crate::support::reference::{CHUNK, SEGMENT_HEADER, TAG};

const FULL: usize = CHUNK + TAG;

fn multi_chunk() -> (RootKey, Vec<u8>, SegmentSeal) {
    let root = root();
    let mut rng = Rng::new(20);
    let mut plain = rng.bytes(300_000);
    plain.extend(rcode_like(&mut rng, 60_000));
    let (sealed, info) = seal(&root, &plain);
    assert!(
        sealed.len() > SEGMENT_HEADER + 3 * FULL,
        "needs at least four chunks"
    );
    (root, sealed, info)
}

fn reseal_id(sealed: &[u8], info: &SegmentSeal) -> SegmentSeal {
    SegmentSeal {
        id: SegmentId(*blake3::hash(sealed).as_bytes()),
        sealed_len: sealed.len() as u64,
        ..*info
    }
}

fn flipped(sealed: &[u8], at: usize) -> Vec<u8> {
    let mut bytes = sealed.to_vec();
    bytes[at] ^= 0x01;
    bytes
}

#[test]
fn one_flipped_byte_anywhere_is_caught_by_the_segment_id() {
    let (root, sealed, info) = multi_chunk();
    for at in (0..sealed.len()).step_by(997).chain(0..SEGMENT_HEADER) {
        assert_eq!(
            open(&root, &flipped(&sealed, at), &info),
            Err(Error::SegmentId),
            "byte {at}"
        );
    }
}

#[test]
fn header_flips_are_caught_by_format_commitment_or_aead_even_with_a_forged_id() {
    let (root, sealed, info) = multi_chunk();
    for at in 0..8 {
        let bad = flipped(&sealed, at);
        assert!(
            matches!(
                open(&root, &bad, &reseal_id(&bad, &info)),
                Err(Error::Format(_))
            ),
            "header byte {at}"
        );
    }
    for at in 8..SEGMENT_HEADER {
        let bad = flipped(&sealed, at);
        assert_eq!(
            open(&root, &bad, &reseal_id(&bad, &info)),
            Err(Error::KeyCommitment),
            "salt or commitment byte {at}"
        );
    }
}

#[test]
fn every_chunk_body_and_tag_flip_fails_authentication_even_with_a_forged_id() {
    let (root, sealed, info) = multi_chunk();
    let chunks = (sealed.len() - SEGMENT_HEADER).div_ceil(FULL);
    for chunk in 0..chunks {
        let start = SEGMENT_HEADER + chunk * FULL;
        let end = (start + FULL).min(sealed.len());
        for at in [
            start,
            start + 1,
            (start + end) / 2,
            end - TAG - 1,
            end - TAG,
            end - 1,
        ] {
            let bad = flipped(&sealed, at);
            assert_eq!(
                open(&root, &bad, &reseal_id(&bad, &info)),
                Err(Error::Authentication),
                "chunk {chunk} byte {at}"
            );
        }
    }
}

#[test]
fn a_flip_in_the_padding_fails_authentication() {
    let (root, sealed, info) = multi_chunk();
    let padded =
        crate::support::reference::open_segment_plaintext(&[9; 32], &sealed).expect("plaintext");
    let compressed_len = u64::from_be_bytes(padded[6..14].try_into().expect("len")) as usize;
    assert!(
        padded.len() > 14 + compressed_len,
        "the segment carries padding"
    );
    let bad = flipped(&sealed, sealed.len() - TAG - 1);
    assert_eq!(
        open(&root, &bad, &reseal_id(&bad, &info)),
        Err(Error::Authentication)
    );
}

#[test]
fn truncation_at_a_chunk_boundary_fails_with_a_forged_id() {
    let (root, sealed, info) = multi_chunk();
    let chunks = (sealed.len() - SEGMENT_HEADER).div_ceil(FULL);
    for keep in 1..chunks {
        let bad = &sealed[..SEGMENT_HEADER + keep * FULL];
        assert_eq!(
            open(&root, bad, &reseal_id(bad, &info)),
            Err(Error::Authentication),
            "kept {keep} chunks"
        );
    }
}

#[test]
fn truncation_inside_a_chunk_fails() {
    let (root, sealed, info) = multi_chunk();
    for cut in [1usize, 15, 16, 17, 100, FULL / 2] {
        let bad = &sealed[..sealed.len() - cut];
        assert!(
            open(&root, bad, &reseal_id(bad, &info)).is_err(),
            "cut {cut}"
        );
    }
    for keep in [
        0usize,
        4,
        SEGMENT_HEADER - 1,
        SEGMENT_HEADER,
        SEGMENT_HEADER + 10,
    ] {
        let bad = &sealed[..keep];
        assert!(
            open(&root, bad, &reseal_id(bad, &info)).is_err(),
            "kept {keep}"
        );
    }
}

#[test]
fn swapping_two_chunks_fails_with_a_forged_id() {
    let (root, sealed, info) = multi_chunk();
    let mut bad = sealed.clone();
    let (a, b) = (SEGMENT_HEADER, SEGMENT_HEADER + FULL);
    let first = sealed[a..a + FULL].to_vec();
    bad[a..a + FULL].copy_from_slice(&sealed[b..b + FULL]);
    bad[b..b + FULL].copy_from_slice(&first);
    assert_eq!(
        open(&root, &bad, &reseal_id(&bad, &info)),
        Err(Error::Authentication)
    );
}

#[test]
fn appending_or_dropping_a_leading_chunk_fails_with_a_forged_id() {
    let (root, sealed, info) = multi_chunk();
    let mut longer = sealed.clone();
    longer.extend_from_slice(&sealed[SEGMENT_HEADER..SEGMENT_HEADER + FULL]);
    assert_eq!(
        open(&root, &longer, &reseal_id(&longer, &info)),
        Err(Error::Authentication)
    );

    let mut shorter = sealed[..SEGMENT_HEADER].to_vec();
    shorter.extend_from_slice(&sealed[SEGMENT_HEADER + FULL..]);
    assert_eq!(
        open(&root, &shorter, &reseal_id(&shorter, &info)),
        Err(Error::Authentication)
    );
}

#[test]
fn a_header_from_another_segment_fails_even_though_it_commits_to_its_own_keys() {
    let (root, sealed, info) = multi_chunk();
    let (other, _) = seal(&root, &rcode_like(&mut Rng::new(21), 1_000));
    let mut bad = other[..SEGMENT_HEADER].to_vec();
    bad.extend_from_slice(&sealed[SEGMENT_HEADER..]);
    assert_eq!(
        open(&root, &bad, &reseal_id(&bad, &info)),
        Err(Error::Authentication)
    );
}

#[test]
fn an_unauthenticated_segment_releases_no_output() {
    let (root, sealed, info) = multi_chunk();
    let bad = flipped(&sealed, sealed.len() - 1);
    let mut out = Vec::new();
    let result = open_segment(
        &root.segment_key(),
        &bad,
        &SegmentExpectation {
            id: reseal_id(&bad, &info).id,
            sealed_len: bad.len() as u64,
            plain_len: info.plain_len,
            dictionary: None,
        },
        &Limits::default(),
        &mut out,
    );
    assert_eq!(result, Err(Error::Authentication));
    assert!(out.is_empty());
}
