//! Shutdown while a client holds an unfinished PUT to the local object route.

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
        !world.object_exists(&pack_id),
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
