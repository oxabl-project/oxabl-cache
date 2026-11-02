//! Two writers, one key: the first commit wins and every reader sees it.

use crate::support::fixture::{World, token};
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use oxabl_cache_format as format;

const TRUSTED: format::Namespace = format::Namespace::Trusted;

#[test]
fn the_first_of_two_tokens_to_commit_a_key_wins_and_the_other_is_told_it_exists() {
    let world = World::start("first-write-wins", Options::default());
    let second = token(
        &world.admin,
        "ci-2",
        &["read", "write:trusted"],
        &[&world.cache],
    );
    let second = world
        .admin
        .with_token(second["token"].as_str().expect("token"));
    let key = frames::lookup_id();

    let first_pack = frames::pack(world.cache_bytes, &[4_000]);
    let first_entry = Entry::new(key, 0);
    let (_, statuses) = world.publish(
        &world.ci,
        TRUSTED,
        &first_pack,
        std::slice::from_ref(&first_entry),
    );
    assert_eq!(statuses, vec![format::CommitStatus::Committed]);

    let second_pack = frames::pack(world.cache_bytes, &[4_100]);
    let (_, statuses) = world.publish(&second, TRUSTED, &second_pack, &[Entry::new(key, 0)]);
    assert_eq!(statuses, vec![format::CommitStatus::Exists]);

    let (bytes, shape) = world.fetch(&world.reader, &[key]);
    let fetched = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
    assert_eq!(fetched.hits.len(), 1);
    let hit = fetched.hits[0].record.as_ref().expect("record");
    assert_eq!(
        hit.record,
        first_entry.record.as_slice(),
        "readers see the winner's record"
    );
    assert_eq!(u64::from(hit.length), 4_000);
    let info = world.info(&world.reader);
    assert_eq!(
        info["quota"]["live_bytes"], 4_000,
        "the loser's segment is not live"
    );
}
