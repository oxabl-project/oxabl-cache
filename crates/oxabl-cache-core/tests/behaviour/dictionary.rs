//! The dictionary pointer and the life of dictionary segments.

use crate::support::{Env, entry, sealed};
use oxabl_cache_core::Error;
use oxabl_cache_core::domain::entry::EntryStatus::Committed;
use oxabl_cache_core::domain::namespace::NamespaceKind::Trusted;
use oxabl_cache_core::domain::segment::SegmentKind;
use oxabl_cache_core::ids::{PackId, SegmentId};
use oxabl_cache_core::usecases::PurgeScope;

const DAY: i64 = 24 * 3600;

async fn with_roster() -> Env {
    let env = Env::new().await;
    env.engine
        .put_roster(
            &env.cache_admin,
            env.cache,
            0,
            env.keys.owner_signed_roster(1, 0),
        )
        .await
        .expect("roster");
    env
}

async fn publish_dictionary(env: &Env, seed: u32) -> (PackId, SegmentId) {
    let built = env.cache_assemble(&[(sealed(seed, 1024), SegmentKind::Dictionary)]);
    let (pack, statuses) = env.publish(&env.ci, Trusted, &built, vec![]).await;
    assert!(statuses.is_empty());
    (pack, built.segments[0].id)
}

#[tokio::test]
async fn a_dictionary_stays_alive_while_the_pointer_names_it() {
    let env = with_roster().await;
    let (pack, id) = publish_dictionary(&env, 500).await;
    env.engine
        .put_dictionary(&env.ci, env.cache, 0, env.keys.ci_pointer(1, 0, id))
        .await
        .expect("pointer");
    let stored = env
        .engine
        .get_dictionary(&env.reader, env.cache)
        .await
        .expect("get");
    assert_eq!((stored.version, stored.segment_id), (1, id));
    assert_eq!(env.info().await.dictionary_version, 1);

    env.clock.advance(60 * DAY);
    let marked = env.engine.gc_mark().await.expect("mark");
    assert_eq!(
        (marked.dictionaries, marked.packs),
        (0, 0),
        "the pointer keeps it alive"
    );
    assert_eq!(env.engine.gc_sweep().await.expect("sweep"), 0);
    assert!(env.object_exists(pack).await);
    assert_eq!(env.info().await.quota.live_bytes, 1024);

    let built = env.artifacts(&[1], 2048);
    let mut named = entry(1, 0);
    named.dict_id = Some(id);
    env.publish(&env.ci, Trusted, &built, vec![named]).await;
    env.engine
        .purge(&env.cache_admin, env.cache, PurgeScope::BelowEpoch(1))
        .await
        .expect("purge");
    assert_eq!(
        env.info().await.quota.live_bytes,
        1024,
        "deleting the last entry does not release a pointed dictionary"
    );
    env.assert_consistent().await;
}

#[tokio::test]
async fn a_dictionary_named_by_an_entry_survives_the_pointer_moving_and_dies_with_its_last_entry() {
    let env = with_roster().await;
    let (old_pack, old) = publish_dictionary(&env, 500).await;
    env.engine
        .put_dictionary(&env.ci, env.cache, 0, env.keys.ci_pointer(1, 0, old))
        .await
        .expect("pointer");

    let built = env.artifacts(&[1], 2048);
    let mut named = entry(1, 0);
    named.dict_id = Some(old);
    let (_, statuses) = env.publish(&env.ci, Trusted, &built, vec![named]).await;
    assert_eq!(statuses, vec![Committed]);

    let (new_pack, new) = publish_dictionary(&env, 501).await;
    env.engine
        .put_dictionary(&env.ci, env.cache, 1, env.keys.ci_pointer(2, 0, new))
        .await
        .expect("move");
    env.clock.advance(3 * DAY);
    env.engine.gc_mark().await.expect("mark");
    env.engine.gc_sweep().await.expect("sweep");
    assert!(
        env.object_exists(old_pack).await,
        "an entry still names the old dictionary"
    );
    assert!(env.object_exists(new_pack).await);

    let dictionaries = env.fetch(&env.reader, &[1], false).await.dictionaries;
    assert_eq!(
        dictionaries.len(),
        1,
        "fetch mode lists the dictionary the entry needs"
    );
    assert_eq!(dictionaries[0].id, old);

    env.engine
        .purge(&env.cache_admin, env.cache, PurgeScope::BelowEpoch(1))
        .await
        .expect("purge");
    env.assert_consistent().await;
    assert_eq!(
        env.info().await.quota.live_bytes,
        1024,
        "only the pointed dictionary is live"
    );
    env.clock.advance(2 * DAY);
    assert_eq!(
        env.engine.gc_sweep().await.expect("sweep"),
        2,
        "the old dictionary's pack and the artifact pack go"
    );
    assert!(!env.object_exists(old_pack).await);
    assert!(
        env.object_exists(new_pack).await,
        "the pointed dictionary stays"
    );
    env.assert_consistent().await;
}

#[tokio::test]
async fn a_dictionary_nothing_references_dies_after_the_grace() {
    let env = with_roster().await;
    let (pack, _) = publish_dictionary(&env, 500).await;
    env.clock.advance(23 * 3600);
    assert_eq!(
        env.engine.gc_mark().await.expect("mark").dictionaries,
        0,
        "inside the grace"
    );
    env.clock.advance(2 * 3600);
    let marked = env.engine.gc_mark().await.expect("mark");
    assert_eq!(
        (marked.dictionaries, marked.packs),
        (1, 0),
        "a publish whose pointer never moved"
    );
    assert_eq!(env.info().await.quota.live_bytes, 0);
    assert!(
        env.object_exists(pack).await,
        "bytes wait for their own grace"
    );
    env.clock.advance(25 * 3600);
    assert_eq!(env.engine.gc_sweep().await.expect("sweep"), 1);
    assert!(!env.object_exists(pack).await);
    env.assert_consistent().await;
}

#[tokio::test]
async fn the_pointer_needs_write_trusted_a_live_dictionary_a_ci_or_owner_signature_and_the_next_version()
 {
    let env = with_roster().await;
    let (_, dictionary) = publish_dictionary(&env, 500).await;
    let artifact = env.artifacts(&[1], 2048);
    env.publish(&env.ci, Trusted, &artifact, vec![entry(1, 0)])
        .await;

    let denied = env
        .engine
        .put_dictionary(
            &env.dev,
            env.cache,
            0,
            env.keys.ci_pointer(1, 0, dictionary),
        )
        .await;
    assert!(matches!(denied, Err(Error::Forbidden(_))));
    let by_stranger = env
        .engine
        .put_dictionary(
            &env.ci,
            env.cache,
            0,
            env.keys.pointer(1, 0, dictionary, &env.keys.stranger),
        )
        .await;
    assert!(matches!(by_stranger, Err(Error::BadRequest(_))));
    let not_a_dictionary = env
        .engine
        .put_dictionary(
            &env.ci,
            env.cache,
            0,
            env.keys.ci_pointer(1, 0, artifact.segments[0].id),
        )
        .await;
    assert!(
        matches!(not_a_dictionary, Err(Error::BadRequest(_))),
        "an artifact segment is not a dictionary"
    );
    let unknown = env
        .engine
        .put_dictionary(
            &env.ci,
            env.cache,
            0,
            env.keys.ci_pointer(1, 0, SegmentId::of_bytes(b"x")),
        )
        .await;
    assert!(matches!(unknown, Err(Error::BadRequest(_))));
    let wrong_epoch = env
        .engine
        .put_dictionary(&env.ci, env.cache, 0, env.keys.ci_pointer(1, 3, dictionary))
        .await;
    assert!(matches!(wrong_epoch, Err(Error::BadRequest(_))));
    let skipped = env
        .engine
        .put_dictionary(&env.ci, env.cache, 0, env.keys.ci_pointer(2, 0, dictionary))
        .await;
    assert!(matches!(skipped, Err(Error::BadRequest(_))));

    env.engine
        .put_dictionary(&env.ci, env.cache, 0, env.keys.ci_pointer(1, 0, dictionary))
        .await
        .expect("valid");
    let stale = env
        .engine
        .put_dictionary(&env.ci, env.cache, 0, env.keys.ci_pointer(1, 0, dictionary))
        .await;
    assert_eq!(stale, Err(Error::PreconditionFailed));
}
