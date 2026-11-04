//! Quota admission and eviction work on live bytes and take personal entries first.

use crate::support::{Env, entry};
use oxabl_cache_core::Config;
use oxabl_cache_core::domain::namespace::NamespaceKind::{Personal, Trusted};
use oxabl_cache_core::domain::principal::Caller;
use oxabl_cache_core::usecases::{PurgeScope, UploadOutcome};

const HARD: u64 = 100_000;
const SEGMENT: usize = 2000;

fn seeds(first: u32, count: u32) -> Vec<u32> {
    (first..first + count).collect()
}

async fn fill(
    env: &Env,
    caller: &Caller,
    kind: oxabl_cache_core::domain::namespace::NamespaceKind,
    first: u32,
    count: u32,
) {
    let built = env.artifacts(&seeds(first, count), SEGMENT);
    let entries = (0..count).map(|i| entry(first + i, i)).collect();
    let (_, statuses) = env.publish(caller, kind, &built, entries).await;
    assert!(
        statuses
            .iter()
            .all(|s| *s == oxabl_cache_core::domain::entry::EntryStatus::Committed)
    );
}

async fn hit_count(env: &Env, caller: &Caller, first: u32, count: u32) -> usize {
    env.hits(caller, &seeds(first, count), false).await.len()
}

#[tokio::test]
async fn eviction_stops_at_the_live_bytes_target_and_evicts_personal_entries_first() {
    let env = Env::with(Config::default(), HARD).await;
    fill(&env, &env.ci, Trusted, 1000, 35).await;
    fill(&env, &env.dev, Personal, 100, 4).await;
    env.clock.advance(24 * 3600);
    fill(&env, &env.dev2, Personal, 200, 4).await;
    env.clock.advance(24 * 3600);
    let dev3 = env
        .user(
            "dev3",
            &[
                oxabl_cache_core::domain::scope::Scope::Read,
                oxabl_cache_core::domain::scope::Scope::WritePersonal,
            ],
        )
        .await;
    fill(&env, &dev3, Personal, 300, 4).await;
    assert_eq!(
        env.info().await.quota.live_bytes,
        94_000,
        "above the 90% watermark"
    );

    let evicted = env.engine.gc_eviction().await.expect("eviction");
    assert_eq!(evicted, 7, "dev's four oldest entries and three of dev2's");
    assert_eq!(
        env.info().await.quota.live_bytes,
        80_000,
        "stops exactly at the 80% target"
    );
    assert_eq!(
        hit_count(&env, &env.dev, 100, 4).await,
        0,
        "the oldest personal namespace goes first"
    );
    assert_eq!(hit_count(&env, &env.dev2, 200, 4).await, 1);
    assert_eq!(
        hit_count(&env, &dev3, 300, 4).await,
        4,
        "newer personal entries survive"
    );
    assert_eq!(
        hit_count(&env, &env.reader, 1000, 35).await,
        35,
        "trusted entries are untouched"
    );
    env.assert_consistent().await;

    assert_eq!(
        env.engine.gc_eviction().await.expect("eviction"),
        0,
        "at the target nothing more goes"
    );
}

#[tokio::test]
async fn eviction_reaches_trusted_entries_only_after_every_personal_one_is_gone() {
    let env = Env::with(Config::default(), HARD).await;
    fill(&env, &env.ci, Trusted, 1000, 20).await;
    env.clock.advance(24 * 3600);
    fill(&env, &env.ci, Trusted, 2000, 25).await;
    env.clock.advance(24 * 3600);
    fill(&env, &env.dev, Personal, 100, 4).await;
    assert_eq!(env.info().await.quota.live_bytes, 98_000);

    env.engine.gc_eviction().await.expect("eviction");
    assert_eq!(env.info().await.quota.live_bytes, 80_000);
    assert_eq!(
        hit_count(&env, &env.dev, 100, 4).await,
        0,
        "all personal entries went first"
    );
    assert_eq!(
        hit_count(&env, &env.reader, 1000, 20).await,
        15,
        "then the oldest trusted entries"
    );
    assert_eq!(hit_count(&env, &env.reader, 2000, 25).await, 25);
    env.assert_consistent().await;
}

#[tokio::test]
async fn a_trusted_upload_evicts_personal_entries_in_its_own_admission() {
    let env = Env::with(Config::default(), HARD).await;
    fill(&env, &env.ci, Trusted, 1000, 25).await;
    fill(&env, &env.dev, Personal, 100, 4).await;
    env.clock.advance(24 * 3600);
    fill(&env, &env.dev2, Personal, 200, 4).await;
    env.clock.advance(24 * 3600);
    let dev3 = env
        .user(
            "dev3",
            &[
                oxabl_cache_core::domain::scope::Scope::Read,
                oxabl_cache_core::domain::scope::Scope::WritePersonal,
            ],
        )
        .await;
    fill(&env, &dev3, Personal, 300, 4).await;
    assert_eq!(env.info().await.quota.live_bytes, 74_000);

    let bigger = env.artifacts(&seeds(5000, 15), SEGMENT);
    let outcome = env
        .reserve(&env.ci, Trusted, &bigger)
        .await
        .expect("reserve");
    assert!(
        matches!(outcome, UploadOutcome::Reserved { .. }),
        "CI is admitted: {outcome:?}"
    );
    let info = env.info().await;
    assert!(
        info.quota.live_bytes + info.quota.pending_bytes <= HARD,
        "the pack fits after the eviction"
    );
    assert_eq!(
        hit_count(&env, &env.dev, 100, 4).await,
        1,
        "the oldest personal entries made room"
    );
    assert_eq!(hit_count(&env, &env.dev2, 200, 4).await, 4);
    assert_eq!(hit_count(&env, &dev3, 300, 4).await, 4);
    assert_eq!(hit_count(&env, &env.reader, 1000, 25).await, 25);
    env.assert_consistent().await;
}

#[tokio::test]
async fn a_trusted_upload_that_cannot_fit_even_alone_is_refused_and_evicts_nothing() {
    let env = Env::with(Config::default(), HARD).await;
    fill(&env, &env.ci, Trusted, 1000, 25).await;
    fill(&env, &env.dev, Personal, 100, 4).await;
    let huge = env.artifacts(&seeds(5000, 60), SEGMENT);
    let outcome = env.reserve(&env.ci, Trusted, &huge).await.expect("reserve");
    assert_eq!(outcome, UploadOutcome::QuotaExceeded);
    assert_eq!(
        hit_count(&env, &env.dev, 100, 4).await,
        4,
        "nothing was evicted for a refused pack"
    );
    env.assert_consistent().await;
}

#[tokio::test]
async fn a_personal_namespace_is_capped_at_a_tenth_of_the_hard_quota() {
    let env = Env::with(Config::default(), HARD).await;
    let first = env.artifacts(&seeds(100, 4), SEGMENT);
    let second = env.artifacts(&seeds(200, 4), SEGMENT);
    let first_outcome = env
        .reserve(&env.dev, Personal, &first)
        .await
        .expect("reserve");
    assert!(matches!(first_outcome, UploadOutcome::Reserved { .. }));
    let pending_refused = env
        .reserve(&env.dev, Personal, &second)
        .await
        .expect("reserve");
    assert_eq!(
        pending_refused,
        UploadOutcome::QuotaExceeded,
        "pending bytes count against the cap"
    );
    let other_user = env
        .reserve(&env.dev2, Personal, &second)
        .await
        .expect("reserve");
    assert!(
        matches!(other_user, UploadOutcome::Reserved { .. }),
        "the cap is per user"
    );
}

#[tokio::test]
async fn stored_bytes_are_capped_at_three_times_the_hard_quota_even_when_live_bytes_are_zero() {
    let env = Env::with(Config::default(), 20_000).await;
    let mut refused = None;
    for round in 0..10u32 {
        let built = env.artifacts(&seeds(round * 10, 4), SEGMENT);
        match env
            .reserve(&env.ci, Trusted, &built)
            .await
            .expect("reserve")
        {
            UploadOutcome::Reserved { pack, url } => {
                env.put_through_url(&url, pack, &built.bytes).expect("put");
                let entries = (0..4).map(|i| entry(round * 10 + i, i)).collect();
                env.commit(&env.ci, pack, Trusted, &built, entries)
                    .await
                    .expect("commit");
                env.engine
                    .purge(&env.cache_admin, env.cache, PurgeScope::BelowEpoch(1))
                    .await
                    .expect("purge");
            }
            UploadOutcome::QuotaExceeded => {
                refused = Some(round);
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    let info = env.info().await;
    assert_eq!(info.quota.live_bytes, 0, "every entry was purged");
    let round = refused.expect("the stored-bytes ceiling refuses an upload");
    assert!(
        info.quota.stored_bytes + 8_248 > 3 * 20_000,
        "refused at round {round} only because of stored bytes"
    );
}
