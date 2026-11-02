//! Concurrent committers of one lookup id: the first commit wins, the rest are told so.

use crate::support::{Env, entry, sealed};
use oxabl_cache_core::domain::entry::EntryStatus;
use oxabl_cache_core::domain::namespace::NamespaceKind::Trusted;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn first_write_wins_under_two_concurrent_committers() {
    let env = Env::new().await;
    for round in 0..20u32 {
        let first = env.artifacts(&[round * 10 + 1], 4096);
        let second = env.artifacts(&[round * 10 + 2], 4096);
        let pack_a = env.upload(&env.ci, Trusted, &first).await;
        let pack_b = env.upload(&env.ci, Trusted, &second).await;
        let mut entry_a = entry(round, 0);
        entry_a.record = sealed(round * 10 + 3, 256);
        let mut entry_b = entry(round, 0);
        entry_b.record = sealed(round * 10 + 4, 256);
        let request_a = env.request(pack_a, Trusted, &first, vec![entry_a.clone()]);
        let request_b = env.request(pack_b, Trusted, &second, vec![entry_b.clone()]);
        let (engine_a, engine_b) = (env.engine.clone(), env.engine.clone());
        let (caller_a, caller_b) = (env.ci.clone(), env.ci.clone());
        let task_a = tokio::spawn(async move { engine_a.commit_pack(&caller_a, request_a).await });
        let task_b = tokio::spawn(async move { engine_b.commit_pack(&caller_b, request_b).await });
        let status_a = task_a.await.expect("join").expect("commit a")[0];
        let status_b = task_b.await.expect("join").expect("commit b")[0];

        let winner_is_a = match (status_a, status_b) {
            (EntryStatus::Committed, EntryStatus::Exists) => true,
            (EntryStatus::Exists, EntryStatus::Committed) => false,
            other => panic!("expected exactly one winner, got {other:?}"),
        };
        let response = env
            .engine
            .lookup(
                &env.reader,
                oxabl_cache_core::usecases::LookupRequest {
                    cache: env.cache,
                    namespaces: oxabl_cache_core::usecases::NamespaceSet {
                        trusted: true,
                        personal: false,
                    },
                    ids: vec![entry_a.lookup_id],
                    records: true,
                    urls: false,
                },
            )
            .await
            .expect("lookup");
        assert_eq!(response.hits.len(), 1, "the id resolves exactly once");
        let expected = if winner_is_a {
            &entry_a.record
        } else {
            &entry_b.record
        };
        assert_eq!(
            response.hits[0].record.as_ref(),
            Some(expected),
            "readers see the winner"
        );
        let winner_pack = if winner_is_a { pack_a } else { pack_b };
        assert_eq!(response.packs[0].pack, winner_pack);
    }
    env.assert_consistent().await;
    assert_eq!(
        env.info().await.quota.live_bytes,
        20 * 4096,
        "only winners are live"
    );
}
