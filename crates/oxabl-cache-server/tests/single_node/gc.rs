//! Garbage collection and compaction on short timers.

use crate::support::fixture::{World, wait_until};
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use oxabl_cache_format as format;
use std::time::Duration;

const TRUSTED: format::Namespace = format::Namespace::Trusted;
const WAIT: Duration = Duration::from_secs(30);

#[test]
fn an_uncommitted_pack_is_swept_after_its_lease_and_its_reservation_is_released() {
    let world = World::start("lease-sweep", Options::default());
    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let slot = world
        .reserve_bytes(&world.ci, "trusted", &pack.bytes)
        .json()["packs"][0]
        .clone();
    assert_eq!(world.put_slot(&slot, &pack.bytes).status, 200);
    let pack_id = slot["pack_id"].as_str().expect("id").to_owned();
    assert!(world.object_exists(&pack_id));
    assert_eq!(
        world.info(&world.ci)["quota"]["pending_bytes"],
        pack.bytes.len()
    );

    assert!(
        wait_until(WAIT, || !world.object_exists(&pack_id)),
        "the object is deleted once the lease has expired"
    );
    assert_eq!(world.info(&world.ci)["quota"]["pending_bytes"], 0);
    let frame = frames::commit(TRUSTED, &pack, &[Entry::new(frames::lookup_id(), 0)]);
    let mut answers = Vec::new();
    assert!(
        wait_until(WAIT, || {
            answers.push(world.commit(&world.ci, &pack_id, &frame).status);
            answers.last() == Some(&404)
        }),
        "a swept pack is not found once its row is gone: {answers:?}"
    );
    assert!(
        !answers.contains(&200),
        "nothing can commit a swept pack: {answers:?}"
    );
}

#[test]
fn compaction_moves_live_segments_and_lookups_keep_returning_the_same_bytes() {
    let world = World::start("compaction", Options::default());
    let sizes = [1_100_000; 4];
    let keys: Vec<[u8; 32]> = (0..5).map(|_| frames::lookup_id()).collect();

    let first = frames::pack(world.cache_bytes, &sizes);
    let entries: Vec<Entry> = (0..4).map(|n| Entry::new(keys[n], n as u32)).collect();
    let (_, statuses) = world.publish(&world.ci, TRUSTED, &first, &entries);
    assert_eq!(statuses, vec![format::CommitStatus::Committed; 4]);

    let second = frames::pack(world.cache_bytes, &sizes);
    let entries = [
        Entry::new(keys[0], 0),
        Entry::new(keys[1], 1),
        Entry::new(keys[2], 2),
        Entry::new(keys[4], 3),
    ];
    let (old_pack, statuses) = world.publish(&world.ci, TRUSTED, &second, &entries);
    assert_eq!(
        statuses,
        vec![
            format::CommitStatus::Exists,
            format::CommitStatus::Exists,
            format::CommitStatus::Exists,
            format::CommitStatus::Committed,
        ]
    );

    let located = |key: [u8; 32]| {
        let (bytes, shape) = world.fetch(&world.reader, &[key]);
        let response = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
        let hit = response.hits[0].record.as_ref().expect("record");
        (
            response.packs[0].pack_id.to_hex(),
            response.packs[0].url.to_owned(),
            hit.offset,
            hit.length,
        )
    };
    assert_eq!(located(keys[4]).0, old_pack);
    assert!(
        wait_until(WAIT, || located(keys[4]).0 != old_pack),
        "compaction moves the live segment into a new pack"
    );

    let (pack, url, offset, length) = located(keys[4]);
    let range = format!("bytes={offset}-{}", offset + u64::from(length) - 1);
    let moved = world.get_object(&url, Some(&range));
    assert_eq!((moved.status, &moved.body), (206, &second.contents[3]));
    for (n, key) in keys[..4].iter().enumerate() {
        let (_, url, offset, length) = located(*key);
        let range = format!("bytes={offset}-{}", offset + u64::from(length) - 1);
        assert_eq!(world.get_object(&url, Some(&range)).body, first.contents[n]);
    }
    assert!(
        wait_until(WAIT, || !world.object_exists(&old_pack)),
        "the emptied pack is deleted after the grace"
    );
    assert!(world.object_exists(&pack));
    assert_eq!(
        world.info(&world.reader)["quota"]["live_bytes"],
        5 * 1_100_000
    );
}
