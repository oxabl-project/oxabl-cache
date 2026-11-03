//! `backup --to FILE` while commits run, then restoring the copy.

use crate::support::fixture::World;
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use crate::support::sqlite::on_sqlite;
use oxabl_cache_format as format;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const TRUSTED: format::Namespace = format::Namespace::Trusted;
const ENTRIES: usize = 3;

fn lookups(world: &World, keys: &[[u8; 32]]) -> usize {
    let (frame, shape) = frames::lookup(keys, false, false, true, false);
    let answered = world.reader.binary("POST", &world.path("/lookup"), &frame);
    assert_eq!(answered.status, 200);
    format::LookupResponse::decode(&answered.body, &shape)
        .expect("OXLR")
        .hits
        .len()
}

#[test]
fn a_backup_taken_while_commits_run_restores_to_a_server_with_whole_packs_only() {
    let mut world = World::start(
        "backup-online",
        on_sqlite(
            "backup-online",
            Options {
                lease: "60s",
                ..Options::default()
            },
        ),
    );
    let committed: Mutex<Vec<Vec<[u8; 32]>>> = Mutex::new(Vec::new());
    let stop = AtomicBool::new(false);
    let destination = world.server.dir.join("copy.db");
    let mut at_backup = 0;
    std::thread::scope(|scope| {
        scope.spawn(|| {
            while !stop.load(Ordering::Relaxed) {
                let pack = frames::pack(world.cache_bytes, &[3_000; ENTRIES]);
                let keys: Vec<[u8; 32]> = (0..ENTRIES).map(|_| frames::lookup_id()).collect();
                let entries: Vec<Entry> = keys
                    .iter()
                    .enumerate()
                    .map(|(index, key)| Entry::new(*key, index as u32))
                    .collect();
                world.publish(&world.ci, TRUSTED, &pack, &entries);
                committed.lock().expect("log").push(keys);
            }
        });
        let started = Instant::now();
        while committed.lock().expect("log").len() < 4 {
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "commits are running"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        at_backup = committed.lock().expect("log").len();
        let output = world
            .server
            .cli(&["backup", "--to", destination.to_str().expect("path")]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let seen = committed.lock().expect("log").len();
        while committed.lock().expect("log").len() < seen + 2 {
            std::thread::sleep(Duration::from_millis(20));
        }
        stop.store(true, Ordering::Relaxed);
    });
    let log = committed.into_inner().expect("log");
    let mode = fs::metadata(&destination)
        .expect("backup file")
        .permissions()
        .mode();
    assert_eq!(
        mode & 0o777,
        0o600,
        "the copy is readable by its owner only"
    );
    assert!(
        !destination.with_extension("db.part").exists(),
        "no staging file is left"
    );
    let again = world
        .server
        .cli(&["backup", "--to", destination.to_str().expect("path")]);
    assert!(
        !again.status.success(),
        "an existing file is not overwritten"
    );
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("already exists"),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );

    assert!(world.server.stop().success());
    let meta = world.server.dir.join("meta.db");
    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{suffix}", meta.display()));
    }
    fs::copy(&destination, &meta).expect("restore");
    world.server.restart();
    world.reconnect();

    let whole: Vec<bool> = log
        .iter()
        .map(|keys| match lookups(&world, keys) {
            0 => false,
            ENTRIES => true,
            partial => panic!("a pack is half restored: {partial} of {ENTRIES} entries"),
        })
        .collect();
    assert!(
        whole[..at_backup].iter().all(|restored| *restored),
        "every pack committed before the backup began is in the copy"
    );
    assert!(
        !whole.iter().all(|restored| *restored),
        "the packs committed after the backup are not"
    );
    let pack = frames::pack(world.cache_bytes, &[3_000; ENTRIES]);
    let per_pack: u64 = pack
        .segments
        .iter()
        .map(|segment| u64::from(segment.length))
        .sum();
    let restored = whole.iter().filter(|restored| **restored).count() as u64;
    assert_eq!(
        world.info(&world.reader)["quota"]["live_bytes"],
        restored * per_pack,
        "the restored counters match the restored packs"
    );
    let scrub = world.server.cli(&["scrub"]);
    let line: serde_json::Value = serde_json::from_slice(&scrub.stdout).expect("scrub line");
    assert_eq!(
        line["corrupt_segments"], 0,
        "the restored index names intact packs"
    );

    let keys = frames::lookup_id();
    let pack = frames::pack(world.cache_bytes, &[3_000]);
    world.publish(&world.ci, TRUSTED, &pack, &[Entry::new(keys, 0)]);
    assert_eq!(
        lookups(&world, &[keys]),
        1,
        "the restored server takes new writes"
    );
}
