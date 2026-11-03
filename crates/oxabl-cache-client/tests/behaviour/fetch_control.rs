//! What a fetch does when URLs expire, when it is dropped, and when producers or the stage
//! are hostile.

use std::fs;
use std::os::unix::fs::symlink;
use std::time::{Duration, Instant};

use oxabl_cache_client::{Durability, FetchOptions, FetchOutcome, MissReason, Mode};
use oxabl_cache_format::{CacheId, Namespace};

use crate::support::fixture::{self, Set};
use crate::support::forge::Forgery;
use crate::support::proxy::ObjectRule;
use crate::support::server::Timers;
use crate::support::world::{Machine, World, assert_installed, identity_of, tree};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_expired_url_is_renewed_once_and_the_read_succeeds() {
    let world = World::start_with(&Timers {
        url_lifetime: "1s",
        ..Timers::default()
    })
    .await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 4, "expiry");
    ci.store(&sets).await;
    let reader = world.machine(&world.dev, Mode::Developer).await;
    world.proxy.delay_first_object(Duration::from_millis(1_600));
    let outcomes = reader.fetch(&sets).await;
    assert!(
        outcomes
            .iter()
            .all(|o| matches!(o, FetchOutcome::Installed { .. })),
        "{outcomes:?}"
    );
    assert_installed(&reader.stage(), &sets);
    let renewals = world
        .proxy
        .seen()
        .iter()
        .filter(|s| s.method == "POST" && s.path.ends_with("/urls"))
        .count();
    assert_eq!(renewals, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn producer_paths_that_break_the_rules_are_misses_and_the_stage_is_untouched() {
    let world = World::start().await;
    let holder = identity_of(&world.ci);
    let reader = world.machine(&world.dev, Mode::Developer).await;
    let hostile: [&[&str]; 5] = [
        &["a\\b.r", "c.r"],
        &["./x.r", "y.r"],
        &["C:/x.r", "y.r"],
        &["a//b.r", "y.r"],
        &["dir/", "y.r"],
    ];
    let mut sets: Vec<Set> = Vec::new();
    for (n, paths) in hostile.iter().enumerate() {
        let mut set = fixture::sets(&reader.workspace(), 2, "paths")
            .into_iter()
            .find(|s| s.files.len() == 2)
            .expect("a two-file set");
        set.key = fixture::key(900 + n);
        let forgery = Forgery {
            world: &world,
            holder: &holder,
            signer: &holder,
            namespace: Namespace::Trusted,
            token: &world.ci.token,
            target: &world.cache,
            record_cache: CacheId::from_text(&world.cache).expect("id"),
            paths: Some(paths.iter().map(|p| (*p).to_owned()).collect()),
        };
        forgery.publish(std::slice::from_ref(&set)).await;
        sets.push(set);
    }
    let outside = reader.dirs.path().join("outside");
    fs::create_dir_all(&outside).expect("directory");
    let outcomes = reader.fetch(&sets).await;
    assert_eq!(outcomes.len(), sets.len());
    for outcome in &outcomes {
        assert!(
            matches!(
                outcome,
                FetchOutcome::Miss {
                    reason: MissReason::Corrupt,
                    ..
                }
            ),
            "{outcome:?}"
        );
    }
    assert!(tree(&reader.stage()).is_empty());
    assert!(tree(&outside).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_symlink_in_the_stage_cannot_redirect_an_install() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 4, "symlink");
    ci.store(&sets).await;
    let reader = world.machine(&world.dev, Mode::Developer).await;
    let outside = reader.dirs.path().join("outside");
    fs::create_dir_all(&outside).expect("directory");
    fs::create_dir_all(reader.stage()).expect("stage");
    symlink(&outside, reader.stage().join("r")).expect("link");
    let outcomes = reader.fetch(&sets).await;
    for outcome in &outcomes {
        assert!(
            matches!(
                outcome,
                FetchOutcome::Miss {
                    reason: MissReason::Install(_),
                    ..
                }
            ),
            "{outcome:?}"
        );
    }
    assert!(
        tree(&outside).is_empty(),
        "nothing was written through the link"
    );
    assert!(tree(&reader.stage()).is_empty());
}

async fn two_packs(world: &World, slow: ObjectRule) -> (Machine, Vec<Set>, Vec<Set>) {
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let first = fixture::sets(&ci.workspace(), 1, "drop-a");
    ci.store(&first).await;
    let before = world.server.pack_files();
    let second = vec![fixture::single(&ci.workspace(), "drop-b", 1_500_000)];
    ci.store(&second).await;
    let later = world
        .server
        .pack_files()
        .into_iter()
        .find(|p| !before.contains(p))
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .expect("the second pack");
    world.proxy.objects(match slow {
        ObjectRule::Stall { after, .. } => ObjectRule::Stall {
            after,
            only: Some(later),
        },
        ObjectRule::Pace { chunk, every, .. } => ObjectRule::Pace {
            chunk,
            every,
            only: Some(later),
        },
        other => other,
    });
    let reader = world.machine(&world.dev, Mode::Developer).await;
    (reader, first, second)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropping_a_fetch_stops_the_work_and_leaves_completed_installs_alone() {
    let world = World::start().await;
    let (reader, first, second) = two_packs(
        &world,
        ObjectRule::Pace {
            chunk: 2 * 1024,
            every: Duration::from_millis(100),
            only: None,
        },
    )
    .await;
    let both = [first.clone(), second].concat();
    let requests = both
        .iter()
        .map(|s| oxabl_cache_client::FetchRequest {
            key: s.key,
            expect: None,
        })
        .collect();
    let mut options = FetchOptions::new(reader.stage());
    options.durability = Durability::Caller;
    let mut fetch = reader.cache.fetch(requests, options);
    let outcome = tokio::time::timeout(Duration::from_secs(10), fetch.next())
        .await
        .expect("the first outcome arrives while the second pack crawls")
        .expect("an outcome");
    assert!(
        matches!(outcome, FetchOutcome::Installed { key, .. } if key == first[0].key),
        "{outcome:?}"
    );
    let mut waited = 0;
    while world.proxy.streaming() == 0 && waited < 100 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        waited += 1;
    }
    assert!(
        world.proxy.streaming() > 0,
        "the second pack is still being sent"
    );
    drop(fetch);
    let mut waited = 0;
    while world.proxy.streaming() > 0 && waited < 50 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        waited += 1;
    }
    assert_eq!(
        world.proxy.streaming(),
        0,
        "dropping the fetch closes its transfers"
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_installed(&reader.stage(), &first);
    assert_eq!(
        tree(&reader.stage()).len(),
        first[0].files.len(),
        "the work of the dropped fetch never finished"
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn outcomes_arrive_as_keys_complete_not_at_the_end() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 40, "streaming");
    ci.store(&sets).await;
    let reader = world.machine(&world.dev, Mode::Developer).await;
    world.proxy.objects(ObjectRule::Pace {
        chunk: 8 * 1024,
        every: Duration::from_millis(40),
        only: None,
    });
    let requests = sets
        .iter()
        .map(|s| oxabl_cache_client::FetchRequest {
            key: s.key,
            expect: None,
        })
        .collect();
    let started = Instant::now();
    let mut options = FetchOptions::new(reader.stage());
    options.durability = Durability::Caller;
    let mut fetch = reader.cache.fetch(requests, options);
    let mut first = None;
    let mut count = 0;
    while let Some(_outcome) = fetch.next().await {
        first.get_or_insert(started.elapsed());
        count += 1;
    }
    let total = started.elapsed();
    assert_eq!(count, 40);
    assert!(
        first.expect("an outcome") * 2 < total,
        "the first outcome came after {first:?} of {total:?}"
    );
}
