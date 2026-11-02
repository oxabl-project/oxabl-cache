//! Dead packs are deleted only after the grace.

use crate::support::{Env, entry};
use oxabl_cache_core::domain::entry::EntryStatus::{Committed, Exists};
use oxabl_cache_core::domain::namespace::NamespaceKind::Trusted;

#[tokio::test]
async fn a_pack_whose_entries_all_lost_is_dead_at_birth_and_swept_after_the_grace() {
    let env = Env::new().await;
    let winner = env.artifacts(&[1], 4096);
    let (winner_pack, statuses) = env
        .publish(&env.ci, Trusted, &winner, vec![entry(1, 0)])
        .await;
    assert_eq!(statuses, vec![Committed]);
    let loser = env.artifacts(&[2], 4096);
    let (loser_pack, statuses) = env
        .publish(&env.ci, Trusted, &loser, vec![entry(1, 0)])
        .await;
    assert_eq!(statuses, vec![Exists]);
    let loser_size = loser.bytes.len() as u64;
    let winner_size = winner.bytes.len() as u64;
    assert_eq!(
        env.info().await.quota.stored_bytes,
        winner_size + loser_size
    );
    assert_eq!(
        env.info().await.quota.reclaimable_bytes,
        loser_size,
        "a pack in grace is reclaimable whole; the live pack's framing is not"
    );

    assert_eq!(
        env.engine.gc_sweep().await.expect("sweep"),
        0,
        "nothing is due at once"
    );
    assert!(env.object_exists(loser_pack).await);

    env.clock.advance(23 * 3600);
    assert_eq!(
        env.engine.gc_sweep().await.expect("sweep"),
        0,
        "still inside the grace"
    );
    assert!(env.object_exists(loser_pack).await);

    env.clock.advance(2 * 3600);
    assert_eq!(
        env.engine.gc_sweep().await.expect("sweep"),
        1,
        "past the grace the pack goes"
    );
    assert!(!env.object_exists(loser_pack).await, "its bytes are gone");
    assert!(
        env.object_exists(winner_pack).await,
        "the winner's pack stays"
    );
    assert_eq!(env.info().await.quota.stored_bytes, winner_size);
    assert_eq!(env.hits(&env.reader, &[1], false).await, vec![0]);
    env.assert_consistent().await;
}

#[tokio::test]
async fn a_live_pack_is_never_swept() {
    let env = Env::new().await;
    let built = env.artifacts(&[1, 2], 4096);
    let (pack, _) = env
        .publish(&env.ci, Trusted, &built, vec![entry(1, 0), entry(2, 1)])
        .await;
    env.clock.advance(400 * 24 * 3600);
    env.engine.gc_mark().await.expect("mark");
    assert_eq!(env.engine.gc_sweep().await.expect("sweep"), 0);
    assert!(env.object_exists(pack).await);
    assert_eq!(env.hits(&env.reader, &[1, 2], true).await, vec![0, 1]);
}
