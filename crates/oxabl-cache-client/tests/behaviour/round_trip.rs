//! CI stores a build; another machine fetches it.

use oxabl_cache_client::{
    ArtifactPath, Durability, FetchOptions, FetchOutcome, FetchRequest, MissReason, Mode, Presence,
    StoreOutcome, Tier,
};

use crate::support::fixture::{self, key};
use crate::support::world::{World, assert_installed, tree};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ci_stores_a_build_and_a_second_machine_fetches_every_key_byte_identical() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 60, "round-trip");
    let report = ci.store(&sets).await;
    assert_eq!(report.outcomes.len(), 60);
    assert!(
        report
            .outcomes
            .iter()
            .all(|(_, o)| *o == StoreOutcome::Committed),
        "{:?}",
        report.outcomes
    );
    assert!(report.bytes_uploaded > 0);

    for durability in [Durability::PerFile, Durability::Caller] {
        let dev = world.machine(&world.dev, Mode::Developer).await;
        let mut options = FetchOptions::new(dev.stage());
        options.durability = durability;
        let outcomes = dev.fetch_with(&sets, options).await;
        assert_eq!(outcomes.len(), 60);
        for outcome in &outcomes {
            let FetchOutcome::Installed {
                provenance, files, ..
            } = outcome
            else {
                panic!("expected a hit, got {outcome:?}");
            };
            assert_eq!(provenance.tier, Tier::Trusted);
            assert_eq!(provenance.signer, world.ci.public);
            assert!(!files.is_empty());
        }
        assert_installed(&dev.stage(), &sets);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_predicts_presence_and_a_second_store_reports_existed() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 5, "probe");
    ci.store(&sets).await;

    let dev = world.machine(&world.dev, Mode::Developer).await;
    let mut keys: Vec<_> = sets.iter().map(|s| s.key).collect();
    keys.push(key(10_000));
    let presence = dev.cache.probe(&keys).await.expect("probe");
    assert_eq!(&presence[..5], &[Presence::Hit; 5]);
    assert_eq!(presence[5], Presence::Miss);

    let again = ci.store(&sets).await;
    assert!(
        again
            .outcomes
            .iter()
            .all(|(_, o)| *o == StoreOutcome::Existed)
    );
    assert!(dev.cache.stats().requests > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_expected_path_set_must_match_exactly() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 3, "expect");
    ci.store(&sets).await;
    let dev = world.machine(&world.dev, Mode::Developer).await;

    let request = |set: &fixture::Set, paths: Vec<oxabl_cache_client::ArtifactPath>| FetchRequest {
        key: set.key,
        expect: Some(paths),
    };
    let mut fewer = sets[0].paths();
    fewer.pop();
    let mut more = sets[1].paths();
    more.push(ArtifactPath::new("r/extra.r").expect("path"));
    let requests = vec![
        request(&sets[0], fewer),
        request(&sets[1], more),
        request(&sets[2], sets[2].paths()),
    ];
    let mut fetch = dev.cache.fetch(requests, FetchOptions::new(dev.stage()));
    let mut outcomes = Vec::new();
    while let Some(outcome) = fetch.next().await {
        outcomes.push(outcome);
    }
    for outcome in &outcomes {
        match outcome {
            FetchOutcome::Miss { key, reason } => {
                assert_ne!(*key, sets[2].key);
                assert_eq!(*reason, MissReason::PathMismatch);
            }
            FetchOutcome::Installed { key, .. } => assert_eq!(*key, sets[2].key),
        }
    }
    assert_eq!(outcomes.len(), 3);
    assert_installed(&dev.stage(), &sets[2..]);
    assert_eq!(
        tree(&dev.stage()).len(),
        sets[2].files.len(),
        "nothing of the mismatched sets"
    );
}
