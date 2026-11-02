use oxabl_cache_crypto::{
    Dictionary, Error, SealOptions, SegmentExpectation, open_dictionary, open_record, open_segment,
    seal_dictionary, seal_record, seal_segment, sign_pointer, train, verify_pointer,
};
use oxabl_cache_format::{DictionaryPointer, Limits, LookupId, Namespace};

use crate::support::data::{Rng, rcode_like};
use crate::support::world::{World, fields};

fn samples(seed: u64, count: usize) -> Vec<Vec<u8>> {
    let mut rng = Rng::new(seed);
    (0..count)
        .map(|_| {
            let len = 6_000 + rng.below(20_000) as usize;
            rcode_like(&mut rng, len)
        })
        .collect()
}

fn trained(w: &World, seed: u64) -> (Dictionary, Vec<u8>) {
    let bytes = train(&samples(seed, 300), 16 * 1024).expect("train");
    let mut sealed = Vec::new();
    let info =
        seal_dictionary(&w.root.segment_key(), &bytes, &mut sealed).expect("seal dictionary");
    (
        open_dictionary(&w.root.segment_key(), &sealed, &info.id, 3).expect("open dictionary"),
        sealed,
    )
}

fn compressed_len(w: &World, dict: Option<&Dictionary>, data: &[u8]) -> u64 {
    seal_segment(
        &w.root.segment_key(),
        data,
        &SealOptions {
            level: 3,
            dictionary: dict,
        },
        &Limits::default(),
        Vec::new(),
    )
    .expect("seal")
    .compressed_len
}

#[test]
fn a_trained_dictionary_shrinks_small_sets() {
    let w = World::new();
    let (dict, _) = trained(&w, 70);
    let fresh = samples(71, 40);
    let without: u64 = fresh.iter().map(|d| compressed_len(&w, None, d)).sum();
    let with: u64 = fresh
        .iter()
        .map(|d| compressed_len(&w, Some(&dict), d))
        .sum();
    println!("without {without}, with {with}");
    assert!(
        with * 100 < without * 90,
        "with the dictionary {with} bytes, without {without}"
    );
}

#[test]
fn a_dictionary_travels_as_a_sealed_segment_and_round_trips() {
    let w = World::new();
    let bytes = train(&samples(72, 300), 16 * 1024).expect("train");
    assert!(!bytes.is_empty() && bytes.len() <= 16 * 1024);
    let mut sealed = Vec::new();
    let info = seal_dictionary(&w.root.segment_key(), &bytes, &mut sealed).expect("seal");
    assert_eq!(info.id.0, *blake3::hash(&sealed).as_bytes());
    let dict = open_dictionary(&w.root.segment_key(), &sealed, &info.id, 3).expect("open");
    assert_eq!(dict.as_bytes(), &bytes[..]);
    assert_eq!(dict.id(), info.id);
}

#[test]
fn a_dictionary_segment_is_checked_before_it_is_trusted() {
    let w = World::new();
    let (_, sealed) = trained(&w, 73);
    let id = oxabl_cache_format::SegmentId(*blake3::hash(&sealed).as_bytes());
    let wrong_id = oxabl_cache_format::SegmentId([7; 32]);
    assert!(matches!(
        open_dictionary(&w.root.segment_key(), &sealed, &wrong_id, 3),
        Err(Error::SegmentId)
    ));
    let mut bad = sealed.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    let bad_id = oxabl_cache_format::SegmentId(*blake3::hash(&bad).as_bytes());
    assert!(matches!(
        open_dictionary(&w.root.segment_key(), &bad, &bad_id, 3),
        Err(Error::Authentication)
    ));
    assert!(
        open_dictionary(&World::new().root.segment_key(), &sealed, &id, 3).is_err(),
        "another cache's key"
    );
}

#[test]
fn artifact_and_dictionary_segments_are_not_interchangeable() {
    let w = World::new();
    let (dict, dict_sealed) = trained(&w, 74);
    let data = rcode_like(&mut Rng::new(75), 9_000);
    let mut sealed = Vec::new();
    let info = seal_segment(
        &w.root.segment_key(),
        &data[..],
        &SealOptions::default(),
        &Limits::default(),
        &mut sealed,
    )
    .expect("seal");
    assert!(matches!(
        open_dictionary(&w.root.segment_key(), &sealed, &info.id, 3),
        Err(Error::Mismatch(_))
    ));

    let dict_id = oxabl_cache_format::SegmentId(*blake3::hash(&dict_sealed).as_bytes());
    let result = open_segment(
        &w.root.segment_key(),
        &dict_sealed,
        &SegmentExpectation {
            id: dict_id,
            sealed_len: dict_sealed.len() as u64,
            plain_len: 10,
            dictionary: None,
        },
        &Limits::default(),
        Vec::new(),
    );
    assert!(matches!(result, Err(Error::Mismatch(_))));
    let _ = dict;
}

#[test]
fn a_set_sealed_with_a_dictionary_needs_exactly_that_dictionary() {
    let w = World::new();
    let (dict, _) = trained(&w, 76);
    let (other, _) = trained(&w, 77);
    let data = rcode_like(&mut Rng::new(78), 12_000);
    let mut sealed = Vec::new();
    let info = seal_segment(
        &w.root.segment_key(),
        &data[..],
        &SealOptions {
            level: 3,
            dictionary: Some(&dict),
        },
        &Limits::default(),
        &mut sealed,
    )
    .expect("seal");
    let open_with = |d: Option<&Dictionary>| {
        let mut out = Vec::new();
        let r = open_segment(
            &w.root.segment_key(),
            &sealed,
            &SegmentExpectation {
                id: info.id,
                sealed_len: info.sealed_len,
                plain_len: info.plain_len,
                dictionary: d,
            },
            &Limits::default(),
            &mut out,
        );
        (r, out)
    };
    let (ok, out) = open_with(Some(&dict));
    assert_eq!(ok, Ok(()));
    assert_eq!(out, data);
    assert!(matches!(open_with(None).0, Err(Error::Mismatch(_))));
    assert!(
        matches!(open_with(Some(&other)).0, Err(Error::Compression(_))),
        "a different dictionary"
    );
}

#[test]
fn a_record_names_its_dictionary_and_only_that_one_satisfies_it() {
    let w = World::new();
    let (dict, _) = trained(&w, 79);
    let (other, _) = trained(&w, 80);
    let ctx = w.context(Namespace::Trusted, LookupId([4; 32]));
    let mut f = fields(oxabl_cache_format::SegmentId([1; 32]), 70_000, 1_000, 1);
    f.dict_id = Some(dict.id());
    let sealed =
        seal_record(&w.root.record_key(), &ctx, &f, &w.ci, &Limits::default()).expect("seal");
    let accepted = open_record(&w.root.record_key(), &ctx, &sealed, &Limits::default())
        .expect("open")
        .accept(&w.roster, &w.dev.signing_public())
        .expect("accept");
    assert!(accepted.segment_expectation(Some(&dict)).is_ok());
    assert!(matches!(
        accepted.segment_expectation(None),
        Err(Error::Mismatch(_))
    ));
    assert!(matches!(
        accepted.segment_expectation(Some(&other)),
        Err(Error::Mismatch(_))
    ));
}

#[test]
fn empty_and_oversized_dictionaries_are_refused() {
    let w = World::new();
    assert!(seal_dictionary(&w.root.segment_key(), &[], Vec::new()).is_err());
    assert!(
        seal_dictionary(
            &w.root.segment_key(),
            &vec![0u8; 8 * 1024 * 1024 + 1],
            Vec::new()
        )
        .is_err()
    );
    assert!(train(&samples(81, 3), 0).is_err());
}

fn pointer(w: &World, signer: &oxabl_cache_crypto::Identity, epoch: u32) -> Vec<u8> {
    sign_pointer(
        signer,
        w.cache_id,
        epoch,
        1,
        oxabl_cache_format::SegmentId([9; 32]),
    )
    .encode()
}

#[test]
fn a_dictionary_pointer_is_accepted_from_ci_and_owner_keys_only() {
    let w = World::new();
    for (who, signer) in [("ci", &w.ci), ("owner", &w.owner)] {
        let verified = verify_pointer(&pointer(&w, signer, 1), &w.roster).expect(who);
        assert_eq!(verified.signer, signer.signing_public());
    }
    for (who, signer) in [("developer", &w.dev), ("reader", &w.reader)] {
        assert_eq!(
            verify_pointer(&pointer(&w, signer, 1), &w.roster).map(|_| ()),
            Err(Error::SignerNotAccepted),
            "{who}"
        );
    }
}

#[test]
fn a_dictionary_pointer_for_another_epoch_or_with_a_bad_signature_is_refused() {
    let w = World::new();
    assert!(matches!(
        verify_pointer(&pointer(&w, &w.ci, 2), &w.roster),
        Err(Error::Mismatch(_))
    ));
    let good = pointer(&w, &w.ci, 1);
    for at in 0..good.len() {
        let mut bad = good.clone();
        bad[at] ^= 1;
        assert!(verify_pointer(&bad, &w.roster).is_err(), "byte {at}");
    }
    let decoded = DictionaryPointer::decode(&good).expect("decode");
    assert_eq!(decoded.version, 1);
}
