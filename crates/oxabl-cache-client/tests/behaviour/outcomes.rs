//! Every request of a fetch ends with exactly one outcome, whoever is slow.

use std::collections::HashSet;
use std::time::Duration;

use oxabl_cache_client::{Durability, FetchOptions, FetchOutcome, FetchRequest, MissReason, Mode};

use crate::support::fixture;
use crate::support::proxy::ObjectRule;
use crate::support::world::{World, assert_installed};

/// More requests than the outcome buffer holds.
const MANY: usize = 1_300;

fn requests(keys: impl Iterator<Item = oxabl_cache_client::CompileKey>) -> Vec<FetchRequest> {
    keys.map(|key| FetchRequest { key, expect: None }).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reader_that_stalls_longer_than_the_idle_timeout_loses_no_outcome() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::large(&ci.workspace(), MANY, 2_000, "slow-reader");
    ci.store(&sets).await;
    let reader = world.machine(&world.dev, Mode::Developer).await;
    let mut options = FetchOptions::new(reader.stage());
    options.durability = Durability::Caller;
    options.idle_timeout = Duration::from_millis(4_000);

    let mut fetch = reader
        .cache
        .fetch(requests(sets.iter().map(|s| s.key)), options);
    let mut outcomes = vec![fetch.next().await.expect("the first outcome")];
    tokio::time::sleep(Duration::from_millis(6_000)).await;
    while let Some(outcome) = fetch.next().await {
        outcomes.push(outcome);
    }

    let installed: HashSet<_> = outcomes
        .iter()
        .filter_map(|o| match o {
            FetchOutcome::Installed { key, .. } => Some(key.0),
            FetchOutcome::Miss { .. } => None,
        })
        .collect();
    assert_eq!(outcomes.len(), MANY, "one outcome per request");
    assert_eq!(installed.len(), MANY, "every request installed, each once");
    assert_installed(&reader.stage(), &sets);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_stops_sending_times_out_each_request_once_for_a_fast_reader() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::large(&ci.workspace(), 30, 200_000, "stalled-link");
    ci.store(&sets).await;
    let reader = world.machine(&world.dev, Mode::Developer).await;
    world.proxy.objects(ObjectRule::Stall {
        after: 100,
        only: None,
    });
    let mut options = FetchOptions::new(reader.stage());
    options.durability = Durability::Caller;
    options.idle_timeout = Duration::from_millis(1_000);

    let outcomes = tokio::time::timeout(Duration::from_secs(20), reader.fetch_with(&sets, options))
        .await
        .expect("the stream ends");

    let mut timed_out = HashSet::new();
    for outcome in &outcomes {
        match outcome {
            FetchOutcome::Miss { key, reason } => {
                assert_eq!(*reason, MissReason::Timeout);
                assert!(timed_out.insert(key.0), "a request ended twice");
            }
            FetchOutcome::Installed { .. } => panic!("nothing can arrive: {outcomes:?}"),
        }
    }
    assert_eq!(timed_out.len(), sets.len(), "one outcome per request");
}
