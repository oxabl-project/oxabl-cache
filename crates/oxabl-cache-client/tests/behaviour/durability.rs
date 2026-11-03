//! The order of installation: bytes are durable before a file has its final name, and a
//! killed process leaves only temporary names or complete files.

use std::time::Duration;

use oxabl_cache_client::{FetchOptions, FetchOutcome, FetchRequest, Mode};
use oxabl_cache_e2e::workdir::TestDir;

use crate::support::driver::Fetching;
use crate::support::fixture::{self, Set};
use crate::support::proxy::ObjectRule;
use crate::support::world::World;

/// Final names under `stage`, read while a fetch may still rename or remove its
/// temporary files: an entry that vanishes during the walk is skipped.
fn final_names(stage: &std::path::Path) -> Vec<String> {
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => walk(root, &path, out),
                Ok(_) => {
                    let name = path.strip_prefix(root).expect("inside");
                    out.push(name.to_string_lossy().into_owned());
                }
                Err(_) => {}
            }
        }
    }
    let mut names = Vec::new();
    walk(stage, stage, &mut names);
    names.retain(|name| !name.contains(".oxc-"));
    names
}

fn expected<'a>(sets: &'a [Set], name: &str) -> &'a [u8] {
    &sets
        .iter()
        .flat_map(|s| &s.files)
        .find(|f| f.path == name)
        .unwrap_or_else(|| panic!("{name} was never stored"))
        .bytes
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_installed_outcome_finds_its_files_complete_at_their_final_paths() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 300, "order");
    ci.store(&sets).await;
    let reader = world.machine(&world.dev, Mode::Developer).await;
    world.proxy.objects(ObjectRule::Pace {
        chunk: 64 * 1024,
        every: Duration::from_millis(5),
        only: None,
    });
    let requests = sets
        .iter()
        .map(|s| FetchRequest {
            key: s.key,
            expect: None,
        })
        .collect();
    let mut fetch = reader
        .cache
        .fetch(requests, FetchOptions::new(reader.stage()));
    let mut reported = 0;
    while let Some(outcome) = fetch.next().await {
        let FetchOutcome::Installed { files, .. } = outcome else {
            panic!("{outcome:?}");
        };
        for file in files {
            let bytes = std::fs::read(reader.stage().join(file.path.as_str()))
                .unwrap_or_else(|e| panic!("{} is reported but missing: {e}", file.path));
            assert_eq!(bytes, expected(&sets, file.path.as_str()), "{}", file.path);
        }
        reported += 1;
    }
    assert_eq!(reported, 300);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_killed_fetch_leaves_complete_files_and_temporary_names_only() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 400, "killed");
    ci.store(&sets).await;
    world.proxy.objects(ObjectRule::Pace {
        chunk: 16 * 1024,
        every: Duration::from_millis(20),
        only: None,
    });
    let state = TestDir::new("state").expect("directory");
    let mut seen_progress = false;
    for round in 0..4u64 {
        let stage = TestDir::new("stage").expect("directory");
        let fetching = Fetching::start(
            world.repo(),
            stage.path(),
            400,
            world.environment(&world.dev, state.path()),
        );
        let started = std::time::Instant::now();
        while final_names(stage.path()).is_empty() && started.elapsed() < Duration::from_secs(60) {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        tokio::time::sleep(Duration::from_millis(round * 15)).await;
        fetching.kill();
        let names = final_names(stage.path());
        seen_progress |= !names.is_empty();
        for name in names {
            assert_eq!(
                std::fs::read(stage.path().join(&name)).expect("read"),
                expected(&sets, &name),
                "{name} has its final name but is not complete"
            );
        }
    }
    assert!(seen_progress, "some kill must land after the first batch");
}

/// Large files take long enough to write that a kill right after the first final name
/// appears lands during a write, if a write ever happens under a final name.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fetch_killed_while_large_files_are_written_leaves_no_partial_final_file() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::large(&ci.workspace(), 6, 24 * 1024 * 1024, "large");
    ci.store(&sets).await;
    let state = TestDir::new("state").expect("directory");
    for round in 0..3u64 {
        let stage = TestDir::new("stage").expect("directory");
        let fetching = Fetching::start(
            world.repo(),
            stage.path(),
            sets.len(),
            world.environment(&world.dev, state.path()),
        );
        let started = std::time::Instant::now();
        while final_names(stage.path()).is_empty() && started.elapsed() < Duration::from_secs(60) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        tokio::time::sleep(Duration::from_millis(round * 3)).await;
        fetching.kill();
        let names = final_names(stage.path());
        assert!(
            !names.is_empty(),
            "round {round}: no file reached its final name"
        );
        for name in names {
            assert!(
                std::fs::read(stage.path().join(&name)).expect("read") == expected(&sets, &name),
                "{name} has its final name but is not complete"
            );
        }
    }
}
