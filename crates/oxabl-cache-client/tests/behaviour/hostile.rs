//! A server, or the network, that lies: every wrong byte is a miss and no final path ever
//! holds anything but what a trusted producer stored.

use std::time::{Duration, Instant};

use oxabl_cache_client::{Durability, FetchOptions, FetchOutcome, MissReason, Mode};
use oxabl_cache_format::{CacheId, CommitStatus, Namespace, Role};

use crate::support::fixture::{self, Set};
use crate::support::forge::Forgery;
use crate::support::proxy::{LookupRule, ObjectRule};
use crate::support::setup::{genesis, member};
use crate::support::world::{Machine, World, identity_of, tree};

struct Fixture {
    world: World,
    sets: Vec<Set>,
    reader: Machine,
}

async fn fixture(count: usize) -> Fixture {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), count, "hostile");
    ci.store(&sets).await;
    let reader = world.machine(&world.dev, Mode::Developer).await;
    Fixture {
        world,
        sets,
        reader,
    }
}

fn misses(outcomes: &[FetchOutcome]) -> Vec<MissReason> {
    outcomes
        .iter()
        .filter_map(|o| match o {
            FetchOutcome::Miss { reason, .. } => Some(*reason),
            FetchOutcome::Installed { .. } => None,
        })
        .collect()
}

/// No file in the stage is partial, unverified or temporary: each is exactly what was stored.
fn assert_only_stored_bytes(reader: &Machine, sets: &[Set]) {
    for (name, bytes) in tree(&reader.stage()) {
        assert!(!name.contains(".oxc-"), "a temporary file remains: {name}");
        let stored = sets
            .iter()
            .flat_map(|s| &s.files)
            .find(|f| f.path == name)
            .unwrap_or_else(|| panic!("{name} was never stored"));
        assert_eq!(bytes, stored.bytes, "{name} is not what was stored");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_answers_garbage_gives_misses_and_no_files() {
    let f = fixture(6).await;
    f.world.proxy.lookup(LookupRule::Garbage);
    let outcomes = f.reader.fetch(&f.sets).await;
    assert_eq!(misses(&outcomes), vec![MissReason::Corrupt; 6]);
    assert!(tree(&f.reader.stage()).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_corrupted_segment_is_a_miss_that_is_reported_and_never_partly_installed() {
    let f = fixture(1).await;
    f.world.proxy.objects(ObjectRule::Flip(40));
    let outcomes = f.reader.fetch(&f.sets).await;
    assert_eq!(misses(&outcomes), vec![MissReason::Corrupt]);
    assert!(
        tree(&f.reader.stage()).is_empty(),
        "no file, not even a partial one"
    );

    let reported = wait_for(|| {
        f.world
            .proxy
            .seen()
            .iter()
            .any(|s| s.method == "POST" && s.path.ends_with("/reports"))
    })
    .await;
    assert!(
        reported,
        "the client reports a segment that does not hash to its id"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn corruption_inside_a_busy_range_only_costs_the_segments_it_touches() {
    let f = fixture(30).await;
    f.world.proxy.objects(ObjectRule::Flip(40));
    let outcomes = f.reader.fetch(&f.sets).await;
    assert_eq!(outcomes.len(), 30);
    let bad = misses(&outcomes);
    assert!(
        !bad.is_empty() && bad.iter().all(|r| *r == MissReason::Corrupt),
        "{bad:?}"
    );
    assert_only_stored_bytes(&f.reader, &f.sets);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_truncated_range_is_a_miss_and_leaves_nothing_behind() {
    let f = fixture(3).await;
    f.world.proxy.objects(ObjectRule::Truncate(100));
    let outcomes = f.reader.fetch(&f.sets).await;
    assert!(misses(&outcomes).len() == 3, "{outcomes:?}");
    assert!(tree(&f.reader.stage()).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn swapped_records_and_swapped_segments_never_install() {
    for rule in [LookupRule::SwapRecords, LookupRule::SwapLocations] {
        let f = fixture(6).await;
        f.world.proxy.lookup(rule.clone());
        let outcomes = f.reader.fetch(&f.sets).await;
        let bad = misses(&outcomes);
        assert_eq!(
            bad.len(),
            2,
            "{rule:?}: the two swapped keys miss: {outcomes:?}"
        );
        assert!(bad.iter().all(|r| *r == MissReason::Corrupt), "{rule:?}");
        assert_only_stored_bytes(&f.reader, &f.sets);
        assert_eq!(
            outcomes.len() - bad.len(),
            4,
            "{rule:?}: the others still install"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn records_of_another_cache_are_misses() {
    let world = World::start().await;
    let other = world
        .setup
        .cache(&world.tenant_admin, "other", &world.owner.signing_public())
        .await;
    let other_token = world
        .setup
        .token(
            &world.tenant_admin,
            "other-ci",
            &["read", "write:trusted"],
            &other,
        )
        .await;
    let holder = identity_of(&world.ci);
    let admin = world
        .setup
        .token(
            &world.tenant_admin,
            "other-admin",
            &["read", "cache:admin"],
            &other,
        )
        .await;
    let (_, roster) = genesis(
        &other,
        vec![
            member("owner", Role::Owner, &world.owner),
            member("ci", Role::Ci, &holder),
        ],
        &world.owner,
    );
    world.setup.put_roster(&admin, &other, 1, roster).await;
    let reader = world.machine(&world.dev, Mode::Developer).await;
    let sets = fixture::sets(&reader.workspace(), 4, "other-cache");
    let forgery = Forgery {
        world: &world,
        holder: &holder,
        signer: &holder,
        namespace: Namespace::Trusted,
        token: &other_token,
        target: &other,
        record_cache: CacheId::from_text(&other).expect("id"),
        paths: None,
    };
    assert_eq!(
        forgery.publish(&sets).await,
        vec![CommitStatus::Committed; 4]
    );
    world.proxy.lookup(LookupRule::Redirect {
        from: world.cache.clone(),
        to: other,
        token: other_token,
    });
    let outcomes = reader.fetch(&sets).await;
    assert_eq!(misses(&outcomes), vec![MissReason::Corrupt; 4]);
    assert!(tree(&reader.stage()).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_is_down_gives_misses_fast() {
    let mut f = fixture(10).await;
    f.world.proxy.go_down();
    let started = Instant::now();
    let outcomes = f.reader.fetch(&f.sets).await;
    assert_eq!(misses(&outcomes), vec![MissReason::Unavailable; 10]);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    assert!(f.reader.cache.probe(&[f.sets[0].key]).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_server_that_keeps_sending_is_not_cut_off() {
    let f = fixture(12).await;
    f.world.proxy.objects(ObjectRule::Pace {
        chunk: 4 * 1024,
        every: Duration::from_millis(50),
        only: None,
    });
    let mut options = FetchOptions::new(f.reader.stage());
    options.durability = Durability::Caller;
    options.idle_timeout = Duration::from_millis(1_000);
    let started = Instant::now();
    let outcomes = f.reader.fetch_with(&f.sets, options).await;
    assert!(
        started.elapsed() > Duration::from_millis(2_000),
        "the fetch must outlast the idle timeout to prove anything: {:?}",
        started.elapsed()
    );
    assert!(misses(&outcomes).is_empty(), "{outcomes:?}");
    crate::support::world::assert_installed(&f.reader.stage(), &f.sets);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bytes_of_one_slow_segment_count_as_progress_even_before_any_key_completes() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = vec![fixture::single(&ci.workspace(), "one-slow", 900_000)];
    ci.store(&sets).await;
    let reader = world.machine(&world.dev, Mode::Developer).await;
    world.proxy.objects(ObjectRule::Pace {
        chunk: 4 * 1024,
        every: Duration::from_millis(50),
        only: None,
    });
    let mut options = FetchOptions::new(reader.stage());
    options.durability = Durability::Caller;
    options.idle_timeout = Duration::from_millis(1_000);
    let started = Instant::now();
    let outcomes = reader.fetch_with(&sets, options).await;
    assert!(
        started.elapsed() > Duration::from_millis(1_500),
        "the transfer must outlast the idle timeout to prove anything: {:?}",
        started.elapsed()
    );
    assert!(misses(&outcomes).is_empty(), "{outcomes:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_stalls_times_out_what_is_pending_and_keeps_what_is_installed() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let first = fixture::sets(&ci.workspace(), 1, "stall-a");
    ci.store(&first).await;
    let before = world.server.pack_files();
    let second = fixture::layout(&ci.workspace(), 2, "stall-b", ("r", "xref", "dbg"))
        .into_iter()
        .skip(1)
        .collect::<Vec<_>>();
    ci.store(&second).await;
    let stalled = world
        .server
        .pack_files()
        .into_iter()
        .find(|p| !before.contains(p))
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .expect("the second pack");
    world.proxy.objects(ObjectRule::Stall {
        after: 100,
        only: Some(stalled),
    });

    let reader = world.machine(&world.dev, Mode::Developer).await;
    let mut options = FetchOptions::new(reader.stage());
    options.durability = Durability::Caller;
    options.idle_timeout = Duration::from_millis(1_000);
    let both = [first.clone(), second.clone()].concat();
    let started = Instant::now();
    let outcomes = tokio::time::timeout(Duration::from_secs(20), reader.fetch_with(&both, options))
        .await
        .expect("the stream ends");
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(outcomes.len(), 2);
    for outcome in &outcomes {
        match outcome {
            FetchOutcome::Installed { key, .. } => assert_eq!(*key, first[0].key),
            FetchOutcome::Miss { key, reason } => {
                assert_eq!(*key, second[0].key);
                assert_eq!(*reason, MissReason::Timeout);
            }
        }
    }
    crate::support::world::assert_installed(&reader.stage(), &first);
    let leftover = tree(&reader.stage());
    assert!(
        leftover
            .keys()
            .all(|name| first[0].files.iter().any(|f| &f.path == name))
    );
}

async fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
    for _ in 0..40 {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}
