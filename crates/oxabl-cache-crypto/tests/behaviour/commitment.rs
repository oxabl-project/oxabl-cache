//! AES-GCM does not commit to its key: a ciphertext can be crafted that authenticates under two
//! keys. These tests construct one and show that the key commitment in the header stops it.

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use oxabl_cache_crypto::{Error, RecordContext, RootKey, open_record};
use oxabl_cache_format::{CacheId, FormatError, Limits, LookupId, Namespace};

use crate::support::reference;

const R: u128 = 0xe1 << 120;
const ONE: u128 = 1 << 127;

fn mul(x: u128, y: u128) -> u128 {
    let (mut z, mut v) = (0u128, y);
    for i in 0..128 {
        if (x >> (127 - i)) & 1 == 1 {
            z ^= v;
        }
        v = if v & 1 == 1 { (v >> 1) ^ R } else { v >> 1 };
    }
    z
}

fn pow(a: u128, e: usize) -> u128 {
    (0..e).fold(ONE, |acc, _| mul(acc, a))
}

fn inverse(a: u128) -> u128 {
    let (mut x, mut acc) = (a, ONE);
    for _ in 1..128 {
        x = mul(x, x);
        acc = mul(acc, x);
    }
    acc
}

fn block(bytes: &[u8]) -> u128 {
    let mut b = [0u8; 16];
    b[..bytes.len()].copy_from_slice(bytes);
    u128::from_be_bytes(b)
}

fn gcm(key: &[u8; 32]) -> LessSafeKey {
    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).expect("key"))
}

fn tag_of(key: &[u8; 32], nonce: [u8; 12], aad: &[u8]) -> u128 {
    let mut empty = Vec::new();
    let tag = gcm(key)
        .seal_in_place_separate_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad),
            &mut empty,
        )
        .expect("seal");
    u128::from_be_bytes(tag.as_ref().try_into().expect("tag"))
}

/// `E_K(J0)` and the hash key `H` of AES-GCM, both recovered through the AEAD interface alone.
fn secrets(key: &[u8; 32], nonce: [u8; 12]) -> (u128, u128) {
    let mask = tag_of(key, nonce, &[]);
    let with_zero_block = tag_of(key, nonce, &[0u8; 16]);
    let length_block = 128u128 << 64;
    let h = mul(with_zero_block ^ mask, inverse(length_block));
    (mask, h)
}

fn ghash(h: u128, aad: &[u8], ciphertext: &[u8]) -> u128 {
    let mut acc = 0u128;
    for chunk in aad.chunks(16).chain(ciphertext.chunks(16)) {
        acc = mul(acc ^ block(chunk), h);
    }
    let lengths =
        (u128::from(aad.len() as u64 * 8) << 64) | u128::from(ciphertext.len() as u64 * 8);
    mul(acc ^ lengths, h)
}

/// Ciphertext and tag that authenticate under both keys for the same nonce and associated data.
fn dual_key_message(
    k1: &[u8; 32],
    k2: &[u8; 32],
    nonce: [u8; 12],
    aad: &[u8],
    len: usize,
) -> Vec<u8> {
    let (m1, h1) = secrets(k1, nonce);
    let (m2, h2) = secrets(k2, nonce);
    let mut ciphertext = (0..len).map(|i| (i * 31 + 7) as u8).collect::<Vec<u8>>();
    let free = 0..16;
    ciphertext[free.clone()].fill(0);

    let blocks_after = ciphertext.len().div_ceil(16) - 1 + 1;
    let exponent = blocks_after + 1;
    let rhs = ghash(h1, aad, &ciphertext) ^ ghash(h2, aad, &ciphertext) ^ m1 ^ m2;
    let unknown = mul(rhs, inverse(pow(h1, exponent) ^ pow(h2, exponent)));
    ciphertext[free].copy_from_slice(&unknown.to_be_bytes());
    let tag = ghash(h1, aad, &ciphertext) ^ m1;
    ciphertext.extend_from_slice(&tag.to_be_bytes());
    ciphertext
}

fn opens(key: &[u8; 32], nonce: [u8; 12], aad: &[u8], message: &[u8]) -> bool {
    let mut buf = message.to_vec();
    gcm(key)
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad),
            &mut buf,
        )
        .is_ok()
}

fn two_roots() -> (RootKey, RootKey) {
    (RootKey::from_bytes([1; 32]), RootKey::from_bytes([2; 32]))
}

#[test]
fn the_construction_really_authenticates_under_two_keys() {
    let (k1, k2) = ([1u8; 32], [2u8; 32]);
    let aad = b"associated data of any length at all, 49 bytes...";
    let message = dual_key_message(&k1, &k2, [0; 12], aad, 168);
    assert!(
        opens(&k1, [0; 12], aad, &message),
        "opens under the first key"
    );
    assert!(
        opens(&k2, [0; 12], aad, &message),
        "opens under the second key"
    );
    assert!(
        !opens(&[3; 32], [0; 12], aad, &message),
        "and under no other key"
    );
}

#[test]
fn a_record_crafted_for_two_keys_is_refused_by_the_key_it_does_not_commit_to() {
    let (root1, root2) = two_roots();
    let salt = [6u8; 32];
    let ctx = RecordContext {
        cache_id: CacheId([3; 16]),
        epoch: 4,
        namespace: Namespace::Trusted,
        lookup_id: LookupId([5; 32]),
    };
    let ad = reference::record_ad(
        &ctx.cache_id.0,
        ctx.epoch,
        ctx.namespace.to_u8(),
        &ctx.lookup_id.0,
    );
    let (_, _, record1) = reference::sub_keys(&[1; 32]);
    let (_, _, record2) = reference::sub_keys(&[2; 32]);
    let (enc1, commit1) = reference::object_keys(&record1, &salt);
    let (enc2, _) = reference::object_keys(&record2, &salt);

    let message = dual_key_message(&enc1, &enc2, [0; 12], &ad, 424);
    assert!(opens(&enc1, [0; 12], &ad, &message) && opens(&enc2, [0; 12], &ad, &message));

    let mut sealed = b"OXER".to_vec();
    sealed.extend_from_slice(&[1, 1, 0, 0]);
    sealed.extend_from_slice(&salt);
    sealed.extend_from_slice(&commit1);
    sealed.extend_from_slice(&message);
    assert_eq!(sealed.len(), 512);

    let limits = Limits::default();
    let under_first = open_record(&root1.record_key(), &ctx, &sealed, &limits).map(|_| ());
    assert_eq!(
        under_first,
        Err(Error::Format(FormatError::BadMagic)),
        "the committed key authenticates and reaches the content, which is not a record"
    );
    let under_second = open_record(&root2.record_key(), &ctx, &sealed, &limits).map(|_| ());
    assert_eq!(under_second, Err(Error::KeyCommitment));
}

#[test]
fn a_segment_crafted_for_two_keys_is_refused_by_the_key_it_does_not_commit_to() {
    use oxabl_cache_crypto::{SegmentExpectation, open_segment};
    let (root1, root2) = two_roots();
    let salt = [6u8; 32];
    let (_, segment1, _) = reference::sub_keys(&[1; 32]);
    let (_, segment2, _) = reference::sub_keys(&[2; 32]);
    let (enc1, commit1) = reference::object_keys(&segment1, &salt);
    let (enc2, _) = reference::object_keys(&segment2, &salt);

    let header = reference::segment_header(&salt, &commit1);
    let aad = blake3::hash(&header);
    let nonce = reference::nonce(0, true);
    let message = dual_key_message(&enc1, &enc2, nonce, aad.as_bytes(), 48);
    assert!(
        opens(&enc1, nonce, aad.as_bytes(), &message)
            && opens(&enc2, nonce, aad.as_bytes(), &message)
    );

    let mut sealed = header.to_vec();
    sealed.extend_from_slice(&message);
    let id = oxabl_cache_format::SegmentId(*blake3::hash(&sealed).as_bytes());
    let open_under = |root: &RootKey| {
        open_segment(
            &root.segment_key(),
            &sealed,
            &SegmentExpectation {
                id,
                sealed_len: sealed.len() as u64,
                plain_len: 1,
                dictionary: None,
            },
            &Limits::default(),
            Vec::new(),
        )
    };
    assert_eq!(
        open_under(&root1),
        Err(Error::Format(FormatError::BadMagic)),
        "the committed key authenticates and reaches the content, which is not a segment"
    );
    assert_eq!(open_under(&root2), Err(Error::KeyCommitment));
}
