//! A commit is recorded once; repeats are answered from the record.

use crate::support::{Env, entry};
use oxabl_cache_core::domain::entry::EntryStatus;
use oxabl_cache_core::domain::namespace::NamespaceKind::Trusted;
use oxabl_cache_core::usecases::PurgeScope;
use oxabl_cache_core::{Conflict, Error};

#[tokio::test]
async fn a_retried_commit_returns_the_recorded_answer_even_after_the_entries_die() {
    let env = Env::new().await;
    let built = env.artifacts(&[1, 2, 3], 2048);
    let pack = env.upload(&env.ci, Trusted, &built).await;
    let entries = vec![entry(1, 0), entry(2, 1), entry(3, 2)];
    let first = env
        .commit(&env.ci, pack, Trusted, &built, entries.clone())
        .await
        .expect("commit");
    assert_eq!(first, vec![EntryStatus::Committed; 3]);
    let live = env.info().await.quota.live_bytes;
    assert_eq!(live, 3 * 2048);

    let repeat = env
        .commit(&env.ci, pack, Trusted, &built, entries.clone())
        .await
        .expect("repeat");
    assert_eq!(repeat, first, "a repeat sees exactly the first answer");
    assert_eq!(
        env.info().await.quota.live_bytes,
        live,
        "a repeat changes nothing"
    );

    let purged = env
        .engine
        .purge(&env.cache_admin, env.cache, PurgeScope::BelowEpoch(1))
        .await
        .expect("purge");
    assert_eq!(purged, 3);
    assert_eq!(env.info().await.quota.live_bytes, 0);
    let after_death = env
        .commit(&env.ci, pack, Trusted, &built, entries.clone())
        .await
        .expect("repeat after death");
    assert_eq!(
        after_death, first,
        "the answer survives the death of its segments"
    );

    let other_body = env
        .commit(&env.ci, pack, Trusted, &built, vec![entry(9, 0)])
        .await;
    assert_eq!(
        other_body,
        Err(Error::Conflict(Conflict::PackClosed)),
        "a different body is refused"
    );
    env.assert_consistent().await;

    env.clock.advance(25 * 3600);
    env.engine.gc_sweep().await.expect("sweep");
    let gone = env.commit(&env.ci, pack, Trusted, &built, entries).await;
    assert_eq!(
        gone,
        Err(Error::NotFound),
        "once the pack is swept there is nothing to answer from"
    );
}

#[tokio::test]
async fn a_pack_that_was_replaced_after_commit_is_not_answered_and_is_queued_for_a_scrub() {
    let env = Env::new().await;
    let built = env.artifacts(&[1, 2], 2048);
    let (pack, statuses) = env
        .publish(&env.ci, Trusted, &built, vec![entry(1, 0), entry(2, 1)])
        .await;
    assert_eq!(statuses, vec![EntryStatus::Committed; 2]);

    let path = env.object_path(pack);
    let mut bytes = std::fs::read(&path).expect("read pack");
    let target = usize::try_from(built.segments[0].offset).expect("offset") + 10;
    bytes[target] ^= 0xff;
    std::fs::write(&path, &bytes).expect("rewrite pack");

    let repeat = env
        .commit(
            &env.ci,
            pack,
            Trusted,
            &built,
            vec![entry(1, 0), entry(2, 1)],
        )
        .await;
    assert_eq!(repeat, Err(Error::Conflict(Conflict::PackChanged)));

    let outcome = env.engine.run_queued_scrubs().await.expect("scrub");
    assert_eq!(
        outcome.corrupt_segments, 1,
        "the scrub finds the damaged segment"
    );
    assert_eq!(outcome.entries_removed, 1);
    assert_eq!(
        env.hits(&env.reader, &[1, 2], false).await,
        vec![1],
        "only the intact entry still resolves"
    );
    env.assert_consistent().await;
}

#[tokio::test]
async fn a_commit_before_the_bytes_arrive_or_with_the_wrong_size_is_refused() {
    let env = Env::new().await;
    let built = env.artifacts(&[1], 2048);
    let outcome = env
        .reserve(&env.ci, Trusted, &built)
        .await
        .expect("reserve");
    let oxabl_cache_core::usecases::UploadOutcome::Reserved { pack, .. } = outcome else {
        panic!("expected a reservation");
    };
    let early = env
        .commit(&env.ci, pack, Trusted, &built, vec![entry(1, 0)])
        .await;
    assert_eq!(
        early,
        Err(Error::Conflict(Conflict::PackIncomplete)),
        "no bytes yet"
    );

    std::fs::create_dir_all(env.object_path(pack).parent().expect("parent")).expect("dirs");
    std::fs::write(env.object_path(pack), &built.bytes[..built.bytes.len() - 1])
        .expect("short object");
    let short = env
        .commit(&env.ci, pack, Trusted, &built, vec![entry(1, 0)])
        .await;
    assert_eq!(
        short,
        Err(Error::Conflict(Conflict::PackIncomplete)),
        "one byte short"
    );
    assert_eq!(
        env.hits(&env.reader, &[1], false).await,
        Vec::<usize>::new()
    );
}
