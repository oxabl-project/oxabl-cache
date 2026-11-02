//! Retention, batched access days and purges.

use crate::support::{Env, entry};
use oxabl_cache_core::domain::namespace::NamespaceKind::{Personal, Trusted};
use oxabl_cache_core::usecases::PurgeScope;

const DAY: i64 = 24 * 3600;

#[tokio::test]
async fn entries_unread_for_the_retention_period_go_and_read_entries_stay() {
    let env = Env::new().await;
    let built = env.artifacts(&[1, 2], 2048);
    let (pack, _) = env
        .publish(&env.ci, Trusted, &built, vec![entry(1, 0), entry(2, 1)])
        .await;

    env.clock.advance(10 * DAY);
    env.hits(&env.reader, &[1], true).await;
    assert_eq!(
        env.engine.flush_access_days().await.expect("flush"),
        1,
        "one entry touched"
    );
    env.hits(&env.reader, &[1], true).await;
    assert_eq!(
        env.engine.flush_access_days().await.expect("flush"),
        0,
        "at most once per entry per day"
    );

    env.clock.advance(10 * DAY);
    env.hits(&env.reader, &[1], true).await;
    env.engine.flush_access_days().await.expect("flush");

    env.clock.advance(11 * DAY);
    assert_eq!(
        env.engine.gc_retention().await.expect("retention"),
        1,
        "only the unread entry expired"
    );
    assert_eq!(env.hits(&env.reader, &[1, 2], false).await, vec![0]);
    assert_eq!(env.info().await.quota.live_bytes, 2048);
    env.assert_consistent().await;

    env.clock.advance(31 * DAY);
    assert_eq!(env.engine.gc_retention().await.expect("retention"), 1);
    assert_eq!(env.info().await.quota.live_bytes, 0);
    assert!(env.object_exists(pack).await, "bytes wait for the grace");
    env.clock.advance(2 * DAY);
    assert_eq!(env.engine.gc_sweep().await.expect("sweep"), 1);
    assert!(!env.object_exists(pack).await);
    env.assert_consistent().await;
}

#[tokio::test]
async fn hits_that_were_never_flushed_do_not_keep_an_entry_alive() {
    let env = Env::new().await;
    let built = env.artifacts(&[1], 2048);
    env.publish(&env.ci, Trusted, &built, vec![entry(1, 0)])
        .await;
    env.clock.advance(31 * DAY);
    env.hits(&env.reader, &[1], true).await;
    assert_eq!(
        env.engine.gc_retention().await.expect("retention"),
        1,
        "the read had not been written yet"
    );
}

#[tokio::test]
async fn purging_removes_old_epochs_or_one_personal_namespace() {
    let env = Env::new().await;
    let shared = env.artifacts(&[1, 2], 2048);
    env.publish(&env.ci, Trusted, &shared, vec![entry(1, 0), entry(2, 1)])
        .await;
    let mine = env.artifacts(&[3, 4], 2048);
    env.publish(&env.dev, Personal, &mine, vec![entry(3, 0), entry(4, 1)])
        .await;
    let peers = env.artifacts(&[5], 2048);
    env.publish(&env.dev2, Personal, &peers, vec![entry(5, 0)])
        .await;

    let removed = env
        .engine
        .purge(
            &env.cache_admin,
            env.cache,
            PurgeScope::PersonalOf(env.dev.principal),
        )
        .await
        .expect("purge");
    assert_eq!(removed, 2);
    assert_eq!(
        env.hits(&env.dev, &[3, 4], false).await,
        Vec::<usize>::new()
    );
    assert_eq!(
        env.hits(&env.dev2, &[5], false).await,
        vec![0],
        "other namespaces are untouched"
    );
    assert_eq!(env.hits(&env.reader, &[1, 2], false).await, vec![0, 1]);

    let removed = env
        .engine
        .purge(&env.cache_admin, env.cache, PurgeScope::BelowEpoch(0))
        .await
        .expect("purge");
    assert_eq!(removed, 0, "epoch 0 is not below epoch 0");
    let removed = env
        .engine
        .purge(&env.cache_admin, env.cache, PurgeScope::BelowEpoch(1))
        .await
        .expect("purge");
    assert_eq!(removed, 3);
    assert_eq!(env.info().await.quota.live_bytes, 0);
    env.assert_consistent().await;
}
