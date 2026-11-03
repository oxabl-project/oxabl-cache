//! Restart and the offline scrub.

use crate::support::fixture::World;
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use oxabl_cache_format as format;

const TRUSTED: format::Namespace = format::Namespace::Trusted;

#[test]
fn a_restart_keeps_tokens_entries_and_bytes() {
    let mut world = World::start("restart", Options::default());
    let pack = frames::pack(world.cache_bytes, &[5_000, 6_000]);
    let (a, b) = (frames::lookup_id(), frames::lookup_id());
    let entries = [Entry::new(a, 0), Entry::new(b, 1)];
    world.publish(&world.ci, TRUSTED, &pack, &entries);

    world.server.restart();
    world.reconnect();
    let (bytes, shape) = world.fetch(&world.reader, &[a, b]);
    let found = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
    assert_eq!(found.hits.len(), 2);
    let url = found.packs[0].url;
    assert_eq!(world.get_object(url, None).body, pack.bytes);
    assert_eq!(world.info(&world.reader)["quota"]["live_bytes"], 11_000);
}

#[test]
fn the_scrub_subcommand_finds_corruption_and_removes_the_entries_that_name_it() {
    let mut world = World::start("scrub-cli", Options::default());
    let pack = frames::pack(world.cache_bytes, &[4_000, 5_000]);
    let (a, b) = (frames::lookup_id(), frames::lookup_id());
    let (pack_id, _) = world.publish(
        &world.ci,
        TRUSTED,
        &pack,
        &[Entry::new(a, 0), Entry::new(b, 1)],
    );
    assert!(world.server.stop().success());

    let intact = world.server.cli(&["scrub", "--cache", &world.cache]);
    assert!(
        intact.status.success(),
        "{}",
        String::from_utf8_lossy(&intact.stderr)
    );
    let line: serde_json::Value =
        serde_json::from_slice(&intact.stdout).expect("scrub prints a JSON line");
    assert_eq!(line["corrupt_segments"], 0);

    let at = usize::try_from(pack.segments[1].offset).expect("offset") + 11;
    world.flip_stored_bit(&pack_id, at, 0x01);
    let scrubbed = world.server.cli(&["scrub"]);
    let line: serde_json::Value =
        serde_json::from_slice(&scrubbed.stdout).expect("scrub prints a JSON line");
    assert_eq!(
        (
            line["corrupt_segments"].as_u64(),
            line["entries_removed"].as_u64()
        ),
        (Some(1), Some(1))
    );
    let unknown = world.server.cli(&["scrub", "--cache", "nonsense"]);
    assert!(!unknown.status.success());

    world.server.restart();
    world.reconnect();
    let (bytes, shape) = world.fetch(&world.reader, &[a, b]);
    let found = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
    assert_eq!(
        found.hits.len(),
        1,
        "only the entry of the intact segment is served"
    );
    assert_eq!(found.hits[0].index, 0);
}
