use oxabl_cache_crypto::{LookupKey, ObjectKeys, RecordKey, RootKey, SegmentKey};
use zeroize::{Zeroize, ZeroizeOnDrop};

fn assert_zeroizes_on_drop<T: ZeroizeOnDrop>() {}

#[test]
fn lookup_ids_are_the_keyed_hash_of_the_compile_key_under_the_derived_lookup_key() {
    let root = RootKey::from_bytes([9; 32]);
    let compile_key = [0xab; 32];
    let lookup_key = blake3::derive_key("oxabl-cache v1 lookup", &[9; 32]);
    let expected = blake3::keyed_hash(&lookup_key, &compile_key);
    assert_eq!(
        root.lookup_key().lookup_id(&compile_key).0,
        *expected.as_bytes()
    );
}

#[test]
fn lookup_ids_depend_on_the_epoch_key_and_on_the_compile_key() {
    let a = RootKey::from_bytes([1; 32]).lookup_key();
    let b = RootKey::from_bytes([2; 32]).lookup_key();
    let (k1, k2) = ([5u8; 32], [6u8; 32]);
    assert_eq!(a.lookup_id(&k1), a.lookup_id(&k1));
    assert_ne!(a.lookup_id(&k1), b.lookup_id(&k1));
    assert_ne!(a.lookup_id(&k1), a.lookup_id(&k2));
}

#[test]
fn the_three_sub_keys_are_separated() {
    let root = RootKey::from_bytes([3; 32]);
    let seal = |key: &SegmentKey| {
        let mut out = Vec::new();
        oxabl_cache_crypto::seal_compressed(key, oxabl_cache_format::Codec::None, b"x", &mut out)
            .expect("seal");
        out
    };
    let a = seal(&root.segment_key());
    let b = seal(&SegmentKey::from_bytes(blake3::derive_key(
        "oxabl-cache v1 record",
        &[3; 32],
    )));
    let c = seal(&SegmentKey::from_bytes(blake3::derive_key(
        "oxabl-cache v1 lookup",
        &[3; 32],
    )));
    let commitments = [&a[40..72], &b[40..72], &c[40..72]];
    assert_ne!(commitments[0], commitments[1]);
    assert_ne!(commitments[0], commitments[2]);
    assert_ne!(commitments[1], commitments[2]);
    let reference_commit = {
        let sub = blake3::derive_key("oxabl-cache v1 segment", &[3; 32]);
        let mut material = [0u8; 64];
        material[..32].copy_from_slice(&sub);
        material[32..].copy_from_slice(&a[8..40]);
        blake3::derive_key("oxabl-cache v1 commit", &material)
    };
    assert_eq!(a[40..72], reference_commit);
}

#[test]
fn secret_types_zeroize_on_drop_and_on_demand() {
    assert_zeroizes_on_drop::<RootKey>();
    assert_zeroizes_on_drop::<LookupKey>();
    assert_zeroizes_on_drop::<SegmentKey>();
    assert_zeroizes_on_drop::<RecordKey>();
    assert_zeroizes_on_drop::<ObjectKeys>();
    assert_zeroizes_on_drop::<oxabl_cache_crypto::Identity>();

    let mut root = RootKey::from_bytes([7; 32]);
    let before = root.lookup_key().lookup_id(&[1; 32]);
    root.zeroize();
    let after = root.lookup_key().lookup_id(&[1; 32]);
    assert_ne!(before, after);
    assert_eq!(
        after,
        RootKey::from_bytes([0; 32])
            .lookup_key()
            .lookup_id(&[1; 32]),
        "the key is all zero bytes"
    );
}

#[test]
fn secret_types_never_print_their_contents() {
    let root = RootKey::from_bytes([0x5a; 32]);
    for shown in [
        format!("{root:?}"),
        format!("{:?}", root.lookup_key()),
        format!("{:?}", root.segment_key()),
        format!("{:?}", root.record_key()),
        format!("{:?}", root.segment_key().object_keys(&[0; 32])),
    ] {
        assert!(shown.contains("redacted"), "{shown}");
        assert!(!shown.contains("5a") && !shown.contains("90"), "{shown}");
    }
}
