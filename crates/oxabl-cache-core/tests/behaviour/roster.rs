//! Publishing rosters: versions, epochs and owner signatures, checked with public keys only.

use crate::support::{Env, entry};
use oxabl_cache_core::Error;
use oxabl_cache_core::domain::entry::EntryStatus::{Committed, Invalid};
use oxabl_cache_core::domain::namespace::NamespaceKind::Trusted;

#[tokio::test]
async fn an_owner_signed_roster_is_accepted_and_readable_by_readers() {
    let env = Env::new().await;
    assert_eq!(
        env.engine.get_roster(&env.reader, env.cache).await.err(),
        Some(Error::NotFound)
    );
    let roster = env.keys.owner_signed_roster(1, 0);
    env.engine
        .put_roster(&env.cache_admin, env.cache, 0, roster.clone())
        .await
        .expect("put");
    let stored = env
        .engine
        .get_roster(&env.reader, env.cache)
        .await
        .expect("get");
    assert_eq!(
        (stored.version, stored.epoch, stored.bytes),
        (1, 0, roster.bytes)
    );
    assert_eq!(env.info().await.roster_version, 1);
}

#[tokio::test]
async fn a_stale_if_match_or_a_skipped_version_or_a_jumped_epoch_is_refused() {
    let env = Env::new().await;
    env.engine
        .put_roster(
            &env.cache_admin,
            env.cache,
            0,
            env.keys.owner_signed_roster(1, 0),
        )
        .await
        .expect("v1");

    let stale = env
        .engine
        .put_roster(
            &env.cache_admin,
            env.cache,
            0,
            env.keys.owner_signed_roster(2, 0),
        )
        .await;
    assert_eq!(stale, Err(Error::PreconditionFailed));
    let skipped = env
        .engine
        .put_roster(
            &env.cache_admin,
            env.cache,
            1,
            env.keys.owner_signed_roster(3, 0),
        )
        .await;
    assert!(matches!(skipped, Err(Error::BadRequest(_))));
    let jumped = env
        .engine
        .put_roster(
            &env.cache_admin,
            env.cache,
            1,
            env.keys.owner_signed_roster(2, 2),
        )
        .await;
    assert!(
        matches!(jumped, Err(Error::BadRequest(_))),
        "an epoch of current + 2 would brick the cache"
    );
    assert_eq!(env.info().await.roster_version, 1, "nothing changed");
}

#[tokio::test]
async fn a_roster_without_a_valid_owner_signature_is_refused() {
    let env = Env::new().await;
    let by_ci = env.keys.roster(1, 0, &env.keys.ci);
    let by_stranger = env.keys.roster(1, 0, &env.keys.stranger);
    let mut tampered = env.keys.owner_signed_roster(1, 0);
    tampered.signed_message.push(0);
    let mut unsigned = env.keys.owner_signed_roster(1, 0);
    unsigned.signatures.clear();
    for roster in [by_ci, by_stranger, tampered, unsigned] {
        let refused = env
            .engine
            .put_roster(&env.cache_admin, env.cache, 0, roster)
            .await;
        assert!(matches!(refused, Err(Error::BadRequest(_))), "{refused:?}");
    }
    let mut huge = env.keys.owner_signed_roster(1, 0);
    huge.bytes = vec![0; 1024 * 1024 + 1];
    assert_eq!(
        env.engine
            .put_roster(&env.cache_admin, env.cache, 0, huge)
            .await,
        Err(Error::TooLarge)
    );
    assert_eq!(env.info().await.roster_version, 0);
}

#[tokio::test]
async fn a_new_epoch_makes_entries_of_the_old_epoch_invalid() {
    let env = Env::new().await;
    env.engine
        .put_roster(
            &env.cache_admin,
            env.cache,
            0,
            env.keys.owner_signed_roster(1, 0),
        )
        .await
        .expect("v1");
    env.engine
        .put_roster(
            &env.cache_admin,
            env.cache,
            1,
            env.keys.owner_signed_roster(2, 1),
        )
        .await
        .expect("rotate");
    assert_eq!(env.info().await.epoch, 1);

    let built = env.artifacts(&[1, 2], 2048);
    let mut current = entry(2, 1);
    current.epoch = 1;
    let (_, statuses) = env
        .publish(&env.ci, Trusted, &built, vec![entry(1, 0), current])
        .await;
    assert_eq!(statuses, vec![Invalid, Committed]);
}
