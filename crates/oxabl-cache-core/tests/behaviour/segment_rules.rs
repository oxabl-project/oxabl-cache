//! Segment ownership, one entry per artifact segment, dictionaries and malformed frames.

use crate::support::{Env, entry, lookup_id, sealed};
use oxabl_cache_core::Error;
use oxabl_cache_core::domain::entry::EntryStatus::{Committed, Invalid, MissingDictionary};
use oxabl_cache_core::domain::namespace::NamespaceKind::{Personal, Trusted};
use oxabl_cache_core::domain::segment::SegmentKind;
use oxabl_cache_core::ids::SegmentId;

#[tokio::test]
async fn a_segment_id_from_another_pack_is_invalid_and_the_trusted_entry_still_resolves() {
    let env = Env::new().await;
    let trusted = env.artifacts(&[1], 2048);
    let (trusted_pack, statuses) = env
        .publish(&env.ci, Trusted, &trusted, vec![entry(1, 0)])
        .await;
    assert_eq!(statuses, vec![Committed]);

    let copy = env.artifacts(&[1], 2048);
    assert_eq!(
        copy.segments[0].id, trusted.segments[0].id,
        "the attacker names the same segment"
    );
    let (_, statuses) = env
        .publish(&env.dev, Personal, &copy, vec![entry(2, 0)])
        .await;
    assert_eq!(
        statuses,
        vec![Invalid],
        "a recorded segment id cannot be claimed again"
    );

    let response = env
        .engine
        .lookup(
            &env.reader,
            oxabl_cache_core::usecases::LookupRequest {
                cache: env.cache,
                namespaces: oxabl_cache_core::usecases::NamespaceSet {
                    trusted: true,
                    personal: true,
                },
                ids: vec![lookup_id(1), lookup_id(2)],
                records: true,
                urls: false,
            },
        )
        .await
        .expect("lookup");
    assert_eq!(response.hits.len(), 1, "the invalid entry does not exist");
    assert_eq!(response.hits[0].index, 0);
    assert_eq!(
        response.packs[0].pack, trusted_pack,
        "the trusted entry still points at its own pack"
    );
    assert_eq!(
        env.info().await.quota.live_bytes,
        2048,
        "the copy added no live bytes"
    );
    env.assert_consistent().await;
}

#[tokio::test]
async fn one_artifact_segment_named_by_two_entries_makes_both_invalid() {
    let env = Env::new().await;
    let built = env.artifacts(&[1, 2], 2048);
    let (_, statuses) = env
        .publish(
            &env.ci,
            Trusted,
            &built,
            vec![entry(1, 0), entry(2, 0), entry(3, 1)],
        )
        .await;
    assert_eq!(statuses, vec![Invalid, Invalid, Committed]);
    assert_eq!(env.hits(&env.reader, &[1, 2, 3], false).await, vec![2]);
    assert_eq!(
        env.info().await.quota.live_bytes,
        2048,
        "only the clean segment is live"
    );
    env.assert_consistent().await;
}

#[tokio::test]
async fn a_segment_no_entry_names_is_dead_at_birth_and_cannot_be_claimed_later() {
    let env = Env::new().await;
    let built = env.artifacts(&[1, 2], 2048);
    let (_, statuses) = env
        .publish(&env.ci, Trusted, &built, vec![entry(1, 0)])
        .await;
    assert_eq!(statuses, vec![Committed]);
    assert_eq!(env.info().await.quota.live_bytes, 2048);

    let again = env.artifacts(&[2], 2048);
    let (_, statuses) = env
        .publish(&env.ci, Trusted, &again, vec![entry(2, 0)])
        .await;
    assert_eq!(statuses, vec![Invalid], "a dead segment is never revived");
    env.assert_consistent().await;
}

#[tokio::test]
async fn dictionary_segments_are_trusted_only_and_entries_cannot_claim_them_as_their_own() {
    let env = Env::new().await;
    let personal = env.cache_assemble(&[(sealed(77, 1024), SegmentKind::Dictionary)]);
    let pack = env.upload(&env.dev, Personal, &personal).await;
    let refused = env
        .commit(&env.dev, pack, Personal, &personal, vec![])
        .await;
    assert!(
        matches!(refused, Err(Error::BadRequest(_))),
        "a personal commit cannot carry a dictionary"
    );

    let trusted = env.cache_assemble(&[
        (sealed(77, 1024), SegmentKind::Dictionary),
        (sealed(78, 2048), SegmentKind::Artifact),
    ]);
    let (_, statuses) = env
        .publish(&env.ci, Trusted, &trusted, vec![entry(1, 0), entry(2, 1)])
        .await;
    assert_eq!(
        statuses,
        vec![Invalid, Committed],
        "an entry cannot name a dictionary as its own segment"
    );
    assert_eq!(
        env.info().await.quota.live_bytes,
        1024 + 2048,
        "the dictionary is live with no entry"
    );
    env.assert_consistent().await;
}

#[tokio::test]
async fn entries_naming_an_unknown_dictionary_or_the_wrong_epoch_or_a_bad_header_are_not_stored() {
    let env = Env::new().await;
    let built = env.artifacts(&[1, 2, 3, 4], 2048);
    let mut missing = entry(1, 0);
    missing.dict_id = Some(SegmentId::of_bytes(b"no such dictionary"));
    let mut wrong_epoch = entry(2, 1);
    wrong_epoch.epoch = 5;
    let mut bad_header = entry(3, 2);
    bad_header.header_ok = false;
    let mut oversized = entry(4, 3);
    oversized.record = vec![0; 5000];
    let (_, statuses) = env
        .publish(
            &env.ci,
            Trusted,
            &built,
            vec![missing, wrong_epoch, bad_header, oversized],
        )
        .await;
    assert_eq!(statuses, vec![MissingDictionary, Invalid, Invalid, Invalid]);
    assert_eq!(
        env.hits(&env.reader, &[1, 2, 3, 4], false).await,
        Vec::<usize>::new()
    );
    assert_eq!(env.info().await.quota.live_bytes, 0);
    env.assert_consistent().await;
}

#[tokio::test]
async fn a_segment_table_that_leaves_the_pack_or_overlaps_is_refused_and_the_pack_stays_open() {
    let env = Env::new().await;
    let built = env.artifacts(&[1, 2], 2048);
    let pack = env.upload(&env.ci, Trusted, &built).await;

    let mut outside = built.clone();
    outside.segments[1].length += 5000;
    let refused = env
        .commit(&env.ci, pack, Trusted, &outside, vec![entry(1, 0)])
        .await;
    assert!(
        matches!(refused, Err(Error::BadRequest(_))),
        "a segment outside the body"
    );

    let mut overlapping = built.clone();
    overlapping.segments[1].offset = overlapping.segments[0].offset + 100;
    let refused = env
        .commit(&env.ci, pack, Trusted, &overlapping, vec![entry(1, 0)])
        .await;
    assert!(
        matches!(refused, Err(Error::BadRequest(_))),
        "overlapping segments"
    );

    let mut in_header = built.clone();
    in_header.segments[0].offset = 4;
    let refused = env
        .commit(&env.ci, pack, Trusted, &in_header, vec![entry(1, 0)])
        .await;
    assert!(
        matches!(refused, Err(Error::BadRequest(_))),
        "a segment inside the header"
    );

    let committed = env
        .commit(
            &env.ci,
            pack,
            Trusted,
            &built,
            vec![entry(1, 0), entry(2, 1)],
        )
        .await;
    assert_eq!(
        committed,
        Ok(vec![Committed, Committed]),
        "a refused frame does not close the pack"
    );
}
