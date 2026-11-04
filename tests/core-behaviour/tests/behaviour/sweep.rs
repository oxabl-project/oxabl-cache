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

#[tokio::test]
async fn reclaimable_bytes_are_the_dead_segments_of_live_packs_and_never_their_framing() {
    let env = Env::new().await;
    let first = env.artifacts(&[1], 4096);
    env.publish(&env.ci, Trusted, &first, vec![entry(1, 0)])
        .await;
    let mixed = env.artifacts(&[2, 3], 4096);
    let (_, statuses) = env
        .publish(&env.ci, Trusted, &mixed, vec![entry(1, 0), entry(2, 1)])
        .await;
    assert_eq!(statuses, vec![Exists, Committed]);

    let quota = env.info().await.quota;
    assert_eq!(quota.live_bytes, 2 * 4096);
    assert_eq!(
        quota.reclaimable_bytes, 4096,
        "the dead segment of a live pack, not the framing"
    );
    let framing = (first.bytes.len() + mixed.bytes.len()) as u64 - 3 * 4096;
    assert_eq!(
        quota.stored_bytes,
        quota.live_bytes + quota.pending_bytes + quota.reclaimable_bytes + framing,
        "stored = live + pending + reclaimable + framing"
    );
}
