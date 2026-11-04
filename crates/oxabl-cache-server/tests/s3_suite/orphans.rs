//! The orphan scan deletes objects no pack row names, and only those.

use crate::support::fixture::{World, hex, wait_until};
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use oxabl_cache_format as format;
use std::time::Duration;

const TRUSTED: format::Namespace = format::Namespace::Trusted;

#[test]
fn the_orphan_scan_deletes_an_object_with_no_pack_row_and_keeps_everything_else() {
    let world = World::start(
        "s3-orphans",
        Options {
            lease: "12s",
            extra: "orphan_scan = \"1s\"",
            ..Options::default()
        },
    );
    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let key = frames::lookup_id();
    let (live, _) = world.publish(&world.ci, TRUSTED, &pack, &[Entry::new(key, 0)]);

    let orphan = hex(&frames::random(16));
    let orphan_key = format!("v1/{}/{orphan}", world.cache);
    let stranger = format!("v1/{}/not-a-pack", world.cache);
    let elsewhere = "other/object".to_owned();
    for name in [&orphan_key, &stranger, &elsewhere] {
        put_key(&world, name);
    }

    assert!(
        wait_until(Duration::from_secs(60), || !world
            .bucket_keys(&orphan_key)
            .contains(&orphan_key)),
        "the orphan is deleted once it is older than twice the lease"
    );
    let keys = world.bucket_keys("");
    assert!(
        keys.contains(&stranger),
        "a key that is no pack id is left alone"
    );
    assert!(keys.contains(&elsewhere), "other prefixes are left alone");
    assert!(world.object_exists(&live), "a pack with a row survives");
    let (bytes, shape) = world.fetch(&world.reader, &[key]);
    let found = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
    assert_eq!(found.hits.len(), 1, "its entry is still served");
}

fn put_key(world: &World, key: &str) {
    crate::support::objects::block_on(world.bucket().put(key, frames::random(256))).expect("put");
}
