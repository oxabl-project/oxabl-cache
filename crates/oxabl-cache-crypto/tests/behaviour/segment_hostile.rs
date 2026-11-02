//! Segments that authenticate (the attacker holds the key) but carry hostile content.

use std::io::Write;

use oxabl_cache_crypto::{Error, SegmentExpectation, open_segment};
use oxabl_cache_format::{Limits, SegmentId};

use crate::segments::root;
use crate::support::data::{Rng, rcode_like};
use crate::support::reference;

fn zstd_frame(data: &[u8], checksum: bool) -> Vec<u8> {
    let mut enc = zstd::stream::write::Encoder::new(Vec::new(), 3).expect("encoder");
    enc.include_checksum(checksum).expect("flag");
    enc.write_all(data).expect("write");
    enc.finish().expect("finish")
}

fn craft(codec: u8, compressed: &[u8]) -> (Vec<u8>, SegmentId) {
    let sealed =
        reference::seal_segment_plaintext(&[9; 32], &[3; 32], &reference::oxsp(codec, compressed));
    let id = SegmentId(*blake3::hash(&sealed).as_bytes());
    (sealed, id)
}

fn try_open(sealed: &[u8], id: SegmentId, plain_len: u64) -> (Result<(), Error>, usize) {
    let mut out = Vec::new();
    let result = open_segment(
        &root().segment_key(),
        sealed,
        &SegmentExpectation {
            id,
            sealed_len: sealed.len() as u64,
            plain_len,
            dictionary: None,
        },
        &Limits::default(),
        &mut out,
    );
    (result, out.len())
}

#[test]
fn a_well_formed_crafted_segment_opens() {
    let data = rcode_like(&mut Rng::new(30), 80_000);
    let (sealed, id) = craft(1, &zstd_frame(&data, true));
    let mut out = Vec::new();
    let r = open_segment(
        &root().segment_key(),
        &sealed,
        &SegmentExpectation {
            id,
            sealed_len: sealed.len() as u64,
            plain_len: data.len() as u64,
            dictionary: None,
        },
        &Limits::default(),
        &mut out,
    );
    assert_eq!(r, Ok(()));
    assert_eq!(out, data);
}

#[test]
fn a_frame_without_a_content_checksum_is_refused() {
    let data = rcode_like(&mut Rng::new(31), 10_000);
    let (sealed, id) = craft(1, &zstd_frame(&data, false));
    assert!(matches!(
        try_open(&sealed, id, data.len() as u64).0,
        Err(Error::Compression(_))
    ));
}

#[test]
fn a_corrupted_frame_is_refused() {
    let data = rcode_like(&mut Rng::new(32), 10_000);
    let mut frame = zstd_frame(&data, true);
    let at = frame.len() - 2;
    frame[at] ^= 0xff;
    let (sealed, id) = craft(1, &frame);
    assert!(matches!(
        try_open(&sealed, id, data.len() as u64).0,
        Err(Error::Compression(_))
    ));
}

#[test]
fn a_decompression_bomb_stops_at_the_signed_length() {
    let bomb = zstd_frame(&vec![0u8; 512 * 1024 * 1024], true);
    assert!(bomb.len() < 100_000, "the frame is small");
    let (sealed, id) = craft(1, &bomb);
    let (result, written) = try_open(&sealed, id, 1_000_000);
    assert!(matches!(result, Err(Error::Compression(_))));
    assert!(written <= 1_000_000, "wrote {written} bytes");
}

#[test]
fn two_frames_in_one_segment_are_refused() {
    let data = rcode_like(&mut Rng::new(33), 5_000);
    let mut frames = zstd_frame(&data, true);
    frames.extend(zstd_frame(&data, true));
    let (sealed, id) = craft(1, &frames);
    assert!(matches!(
        try_open(&sealed, id, 2 * data.len() as u64).0,
        Err(Error::Compression(_))
    ));
}

#[test]
fn bytes_after_the_frame_are_refused() {
    let data = rcode_like(&mut Rng::new(34), 5_000);
    let mut frame = zstd_frame(&data, true);
    frame.extend_from_slice(b"trailing");
    let (sealed, id) = craft(1, &frame);
    assert!(matches!(
        try_open(&sealed, id, data.len() as u64).0,
        Err(Error::Compression(_))
    ));
}

#[test]
fn a_skippable_frame_or_garbage_is_refused() {
    for compressed in [
        &b"\x50\x2a\x4d\x18\x04\x00\x00\x00abcd"[..],
        &b"not zstd at all, but long enough"[..],
    ] {
        let (sealed, id) = craft(1, compressed);
        assert!(matches!(
            try_open(&sealed, id, 4).0,
            Err(Error::Compression(_))
        ));
    }
}

#[test]
fn non_zero_padding_is_refused() {
    let data = rcode_like(&mut Rng::new(35), 20_000);
    let mut plaintext = reference::oxsp(1, &zstd_frame(&data, true));
    assert!(
        plaintext.len() > 14 + zstd_frame(&data, true).len(),
        "the plaintext is padded"
    );
    *plaintext.last_mut().expect("padding") = 1;
    let sealed = reference::seal_segment_plaintext(&[9; 32], &[3; 32], &plaintext);
    let id = SegmentId(*blake3::hash(&sealed).as_bytes());
    assert!(matches!(
        try_open(&sealed, id, data.len() as u64).0,
        Err(Error::Format(_))
    ));
}

#[test]
fn padding_of_the_wrong_length_is_refused() {
    let data = rcode_like(&mut Rng::new(36), 20_000);
    let mut plaintext = reference::oxsp(1, &zstd_frame(&data, true));
    plaintext.extend_from_slice(&[0; 64]);
    let sealed = reference::seal_segment_plaintext(&[9; 32], &[3; 32], &plaintext);
    let id = SegmentId(*blake3::hash(&sealed).as_bytes());
    assert!(matches!(
        try_open(&sealed, id, data.len() as u64).0,
        Err(Error::Format(_))
    ));
}

#[test]
fn a_codec_that_does_not_fit_the_dictionary_expectation_is_refused() {
    let data = rcode_like(&mut Rng::new(37), 5_000);
    for codec in [0u8, 2] {
        let (sealed, id) = craft(codec, &zstd_frame(&data, true));
        assert!(
            matches!(
                try_open(&sealed, id, data.len() as u64).0,
                Err(Error::Mismatch(_))
            ),
            "codec {codec}"
        );
    }
}
