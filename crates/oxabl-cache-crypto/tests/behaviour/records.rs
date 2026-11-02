use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use oxabl_cache_crypto::{
    EntryFields, FileEntry, Identity, RecordContext, RootKey, open_record, seal_record,
};
use oxabl_cache_format::{CacheId, EntryPayload, Limits, LookupId, Namespace, SegmentId};

use crate::support::reference;

pub const ROOT: [u8; 32] = [9; 32];

pub fn root() -> RootKey {
    RootKey::from_bytes(ROOT)
}

pub fn ctx() -> RecordContext {
    RecordContext {
        cache_id: CacheId([3; 16]),
        epoch: 4,
        namespace: Namespace::Trusted,
        lookup_id: LookupId([5; 32]),
    }
}

pub fn sample_fields() -> EntryFields {
    EntryFields {
        segment_id: SegmentId([1; 32]),
        segment_len: 70_000,
        dict_id: Some(SegmentId([2; 32])),
        plain_len: 1_300,
        created_at: 1_793_000_000,
        roster_version: 3,
        files: vec![
            FileEntry {
                path: "r/ar/customer.r".into(),
                size: 1_000,
            },
            FileEntry {
                path: "xref/ar/customer.xml".into(),
                size: 300,
            },
        ],
    }
}

pub fn seal(signer: &Identity, ctx: &RecordContext, fields: &EntryFields) -> Vec<u8> {
    seal_record(
        &root().record_key(),
        ctx,
        fields,
        signer,
        &Limits::default(),
    )
    .expect("seal")
}

pub fn raw_ad(ctx: &RecordContext) -> Vec<u8> {
    reference::record_ad(
        &ctx.cache_id.0,
        ctx.epoch,
        ctx.namespace.to_u8(),
        &ctx.lookup_id.0,
    )
}

#[test]
fn a_record_round_trips_with_its_fields_and_signer() {
    let signer = Identity::generate().expect("identity");
    let sealed = seal(&signer, &ctx(), &sample_fields());
    let opened =
        open_record(&root().record_key(), &ctx(), &sealed, &Limits::default()).expect("open");
    assert_eq!(opened.signer(), signer.signing_public());
    assert_eq!(opened.namespace(), Namespace::Trusted);
}

#[test]
fn sealed_records_are_multiples_of_256_bytes_within_the_limit() {
    let signer = Identity::generate().expect("identity");
    let mut seen = std::collections::BTreeSet::new();
    for files in 0..=16usize {
        for path_len in [1usize, 20, 190] {
            let mut fields = sample_fields();
            fields.dict_id = None;
            fields.files = (0..files)
                .map(|i| FileEntry {
                    path: format!("{}{i:02}", "p".repeat(path_len)),
                    size: 1,
                })
                .collect();
            fields.plain_len = files as u64;
            let sealed = seal(&signer, &ctx(), &fields);
            assert_eq!(sealed.len() % 256, 0, "{files} files of {path_len} bytes");
            assert!(sealed.len() <= 4096);
            seen.insert(sealed.len());
        }
    }
    assert!(seen.len() > 3, "sizes fall into several 256-byte classes");
}

#[test]
fn the_longest_legal_record_fits_the_documented_bound() {
    let signer = Identity::generate().expect("identity");
    let mut fields = sample_fields();
    fields.files = (0..16)
        .map(|i| FileEntry {
            path: format!("{i:02}{}", "x".repeat(190)),
            size: 1,
        })
        .collect();
    fields.plain_len = 16;
    assert_eq!(seal(&signer, &ctx(), &fields).len(), 3_584);
}

#[test]
fn product_and_independent_reference_agree_on_records() {
    let signer = Identity::generate().expect("identity");
    let sealed = seal(&signer, &ctx(), &sample_fields());
    let plaintext =
        reference::open_record_raw(&ROOT, &raw_ad(&ctx()), &sealed).expect("reference opens");
    let payload = EntryPayload::decode(&plaintext, &Limits::default()).expect("payload");
    assert_eq!(payload.signer, signer.signing_public());

    let mut transcript = reference::entry_transcript_prefix(&[3; 16], 4, 0, &[5; 32]);
    transcript.extend(
        payload
            .signed_bytes(&Limits::default())
            .expect("signed bytes"),
    );
    VerifyingKey::from_bytes(&payload.signer)
        .expect("key")
        .verify(&transcript, &Signature::from_bytes(&payload.signature))
        .expect("signature verifies under the independent transcript");
}

#[test]
fn a_record_signed_and_sealed_by_the_reference_opens_in_the_product() {
    use ed25519_dalek::{Signer, SigningKey};
    let key = SigningKey::from_bytes(&[11; 32]);
    let fields = sample_fields();
    let mut payload = EntryPayload {
        segment_id: fields.segment_id,
        segment_len: fields.segment_len,
        dict_id: fields.dict_id,
        plain_len: fields.plain_len,
        created_at: fields.created_at,
        roster_version: fields.roster_version,
        signer: key.verifying_key().to_bytes(),
        files: fields
            .files
            .iter()
            .map(|f| oxabl_cache_format::EntryFile {
                path: &f.path,
                size: f.size,
            })
            .collect(),
        signature: [0; 64],
    };
    let mut transcript = reference::entry_transcript_prefix(&[3; 16], 4, 0, &[5; 32]);
    transcript.extend(
        payload
            .signed_bytes(&Limits::default())
            .expect("signed bytes"),
    );
    payload.signature = key.sign(&transcript).to_bytes();
    let plaintext = payload.encode(&Limits::default()).expect("encode");

    let (_, _, record) = reference::sub_keys(&ROOT);
    let salt = [8u8; 32];
    let (enc, commit) = reference::object_keys(&record, &salt);
    let sealed = reference::seal_record_raw(&enc, &commit, &salt, &raw_ad(&ctx()), &plaintext);
    let opened =
        open_record(&root().record_key(), &ctx(), &sealed, &Limits::default()).expect("open");
    assert_eq!(opened.signer(), key.verifying_key().to_bytes());
}

#[test]
fn each_seal_uses_a_fresh_salt() {
    let signer = Identity::generate().expect("identity");
    let a = seal(&signer, &ctx(), &sample_fields());
    let b = seal(&signer, &ctx(), &sample_fields());
    assert_ne!(a[8..40], b[8..40]);
}

#[test]
fn invalid_fields_are_refused_while_sealing() {
    let signer = Identity::generate().expect("identity");
    let mut bad = sample_fields();
    bad.files[0].path = "../escape.r".into();
    assert!(
        seal_record(
            &root().record_key(),
            &ctx(),
            &bad,
            &signer,
            &Limits::default()
        )
        .is_err()
    );

    let mut bad = sample_fields();
    bad.files.push(bad.files[0].clone());
    assert!(
        seal_record(
            &root().record_key(),
            &ctx(),
            &bad,
            &signer,
            &Limits::default()
        )
        .is_err()
    );

    let mut bad = sample_fields();
    bad.plain_len += 1;
    assert!(
        seal_record(
            &root().record_key(),
            &ctx(),
            &bad,
            &signer,
            &Limits::default()
        )
        .is_err()
    );
}
