use oxabl_cache_crypto::{Error, Identity, RecordContext, RootKey, open_record};
use oxabl_cache_format::{CacheId, EntryPayload, Limits, LookupId, Namespace};

use crate::records::{ROOT, ctx, raw_ad, root, sample_fields, seal};
use crate::support::reference;

fn open(sealed: &[u8], ctx: &RecordContext) -> Result<(), Error> {
    open_record(&root().record_key(), ctx, sealed, &Limits::default()).map(|_| ())
}

fn sealed() -> (Identity, Vec<u8>) {
    let signer = Identity::generate().expect("identity");
    let sealed = seal(&signer, &ctx(), &sample_fields());
    (signer, sealed)
}

fn reseal(plaintext: &[u8], ctx: &RecordContext) -> Vec<u8> {
    let (_, _, record) = reference::object_keys_for_record();
    let salt = [4u8; 32];
    let (enc, commit) = reference::object_keys(&record, &salt);
    reference::seal_record_raw(&enc, &commit, &salt, &raw_ad(ctx), plaintext)
}

fn plaintext_of(sealed: &[u8]) -> Vec<u8> {
    reference::open_record_raw(&ROOT, &raw_ad(&ctx()), sealed).expect("plaintext")
}

#[test]
fn every_single_byte_flip_fails_with_the_error_of_its_region() {
    let (_, sealed) = sealed();
    for at in 0..sealed.len() {
        let mut bad = sealed.clone();
        bad[at] ^= 0x01;
        let result = open(&bad, &ctx());
        match at {
            0..8 => assert!(
                matches!(result, Err(Error::Format(_))),
                "header byte {at}: {result:?}"
            ),
            8..72 => assert_eq!(
                result,
                Err(Error::KeyCommitment),
                "salt or commitment byte {at}"
            ),
            _ => assert_eq!(
                result,
                Err(Error::Authentication),
                "ciphertext, padding or tag byte {at}"
            ),
        }
    }
}

#[test]
fn a_record_has_padding_and_the_padding_is_authenticated() {
    let (_, sealed) = sealed();
    let plaintext = plaintext_of(&sealed);
    assert_eq!(*plaintext.last().expect("padding"), 0);
    let mut bad = sealed.clone();
    let last_ciphertext_byte = bad.len() - 17;
    bad[last_ciphertext_byte] ^= 1;
    assert_eq!(open(&bad, &ctx()), Err(Error::Authentication));
}

#[test]
fn a_record_moved_to_another_slot_does_not_open() {
    let (_, sealed) = sealed();
    assert!(open(&sealed, &ctx()).is_ok());
    let moves: [(&str, RecordContext); 4] = [
        (
            "lookup id",
            RecordContext {
                lookup_id: LookupId([6; 32]),
                ..ctx()
            },
        ),
        ("epoch", RecordContext { epoch: 3, ..ctx() }),
        (
            "cache",
            RecordContext {
                cache_id: CacheId([4; 16]),
                ..ctx()
            },
        ),
        (
            "namespace kind",
            RecordContext {
                namespace: Namespace::Personal,
                ..ctx()
            },
        ),
    ];
    for (what, other) in moves {
        assert_eq!(
            open(&sealed, &other),
            Err(Error::Authentication),
            "moved to another {what}"
        );
    }
}

#[test]
fn a_record_does_not_open_under_another_epoch_key() {
    let (_, sealed) = sealed();
    let other = RootKey::from_bytes([10; 32]);
    assert_eq!(
        open_record(&other.record_key(), &ctx(), &sealed, &Limits::default()).map(|_| ()),
        Err(Error::KeyCommitment)
    );
}

#[test]
fn a_replayed_plaintext_resealed_for_another_slot_fails_the_signature_transcript() {
    let (_, sealed) = sealed();
    let plaintext = plaintext_of(&sealed);
    let moves: [(&str, RecordContext); 4] = [
        (
            "lookup id",
            RecordContext {
                lookup_id: LookupId([6; 32]),
                ..ctx()
            },
        ),
        ("epoch", RecordContext { epoch: 3, ..ctx() }),
        (
            "cache",
            RecordContext {
                cache_id: CacheId([4; 16]),
                ..ctx()
            },
        ),
        (
            "namespace kind",
            RecordContext {
                namespace: Namespace::Personal,
                ..ctx()
            },
        ),
    ];
    for (what, other) in moves {
        let moved = reseal(&plaintext, &other);
        assert_eq!(
            open(&moved, &other),
            Err(Error::Signature),
            "re-sealed for another {what}"
        );
    }
}

#[test]
fn a_forged_signature_or_signer_swap_fails_even_with_valid_encryption() {
    let (_, sealed) = sealed();
    let plaintext = plaintext_of(&sealed);
    let signature_at = EntryPayload::decode(&plaintext, &Limits::default())
        .expect("payload")
        .signed_bytes(&Limits::default())
        .expect("signed bytes")
        .len();
    let mut bad = plaintext.clone();
    bad[signature_at] ^= 1;
    assert_eq!(open(&reseal(&bad, &ctx()), &ctx()), Err(Error::Signature));

    let other = Identity::generate().expect("identity");
    let signer_at = 6 + 32 + 4 + 32 + 8 + 8 + 8;
    let mut swapped = plaintext.clone();
    swapped[signer_at..signer_at + 32].copy_from_slice(&other.signing_public());
    assert_eq!(
        open(&reseal(&swapped, &ctx()), &ctx()),
        Err(Error::Signature)
    );
}

#[test]
fn non_zero_padding_is_refused_even_with_valid_encryption() {
    let (_, sealed) = sealed();
    let mut plaintext = plaintext_of(&sealed);
    *plaintext.last_mut().expect("padding") = 1;
    assert!(matches!(
        open(&reseal(&plaintext, &ctx()), &ctx()),
        Err(Error::Format(_))
    ));
}

#[test]
fn records_of_the_wrong_length_are_refused() {
    let (_, sealed) = sealed();
    assert!(matches!(
        open(&sealed[..sealed.len() - 1], &ctx()),
        Err(Error::Format(_))
    ));
    let mut longer = sealed.clone();
    longer.push(0);
    assert!(matches!(open(&longer, &ctx()), Err(Error::Format(_))));
    let mut aligned = sealed.clone();
    aligned.extend_from_slice(&[0; 256]);
    assert_eq!(open(&aligned, &ctx()), Err(Error::Authentication));
    let mut huge = sealed.clone();
    huge.resize(4_352, 0);
    assert!(matches!(open(&huge, &ctx()), Err(Error::Format(_))));
    assert!(matches!(open(&[], &ctx()), Err(Error::Format(_))));
}
