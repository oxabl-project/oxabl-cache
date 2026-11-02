//! Shutdown and restart.

use crate::support::fixture::World;
use crate::support::frames::{self, Entry};
use crate::support::http::{connect, head_text};
use crate::support::server::Options;
use oxabl_cache_format as format;
use std::io::Write;
use std::time::Duration;

const TRUSTED: format::Namespace = format::Namespace::Trusted;

#[test]
fn a_graceful_shutdown_in_the_middle_of_an_upload_leaves_nothing_an_entry_names() {
    let mut world = World::start("shutdown-mid-upload", Options::default());
    let kept = frames::pack(world.cache_bytes, &[3_000]);
    let kept_key = frames::lookup_id();
    world.publish(&world.ci, TRUSTED, &kept, &[Entry::new(kept_key, 0)]);

    let pack = frames::pack(world.cache_bytes, &[200_000]);
    let slot = world.reserve(&world.ci, "trusted", pack.bytes.len()).json()["packs"][0].clone();
    let pack_id = slot["pack_id"].as_str().expect("id").to_owned();
    let (addr, path) = crate::support::http::split_url(slot["url"].as_str().expect("url"));
    let mut stream = connect(&addr);
    let headers = [("Content-Length", pack.bytes.len().to_string())];
    stream
        .write_all(head_text("PUT", &path, &headers).as_bytes())
        .expect("head");
    stream
        .write_all(&pack.bytes[..pack.bytes.len() / 2])
        .expect("half");
    stream.flush().expect("flush");
    std::thread::sleep(Duration::from_millis(300));

    world.server.sigterm();
    let status = world
        .server
        .wait_exit(Duration::from_secs(15))
        .expect("the server exits although a client holds an unfinished upload");
    assert!(status.success(), "{status:?}");
    drop(stream);

    assert!(
        !world.object_file(&pack_id).exists(),
        "no half-written object is published"
    );
    let leftovers = std::fs::read_dir(world.server.dir.join("objects/tmp"))
        .expect("tmp")
        .count();
    assert_eq!(leftovers, 0, "no temporary file survives the shutdown");

    world.server.restart();
    world.reconnect();
    let frame = frames::commit(TRUSTED, &pack, &[Entry::new(frames::lookup_id(), 0)]);
    let late = world.commit(&world.ci, &pack_id, &frame);
    assert_eq!(
        (late.status, late.json()["error"]["reason"].as_str()),
        (409, Some("pack_incomplete"))
    );
    let (bytes, shape) = world.fetch(&world.reader, &[kept_key]);
    let found = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
    assert_eq!(
        found.hits.len(),
        1,
        "what was committed before is still served"
    );
    assert!(world.server.stderr().contains("\"event\":\"shutdown\""));
}

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
fn the_scrub_subcommand_finds_corruption_on_disk_and_removes_the_entries_that_name_it() {
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

    let mut object = std::fs::read(world.object_file(&pack_id)).expect("object");
    let at = usize::try_from(pack.segments[1].offset).expect("offset") + 11;
    object[at] ^= 0x01;
    std::fs::write(world.object_file(&pack_id), object).expect("tamper");
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
