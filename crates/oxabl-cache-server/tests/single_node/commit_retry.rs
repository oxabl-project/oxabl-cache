//! A commit is applied once; its retry gets the recorded answer.

use crate::support::fixture::World;
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use oxabl_cache_format as format;

const TRUSTED: format::Namespace = format::Namespace::Trusted;

#[test]
fn a_commit_retry_gets_the_recorded_answer_and_a_different_body_is_refused() {
    let world = World::start("commit-retry", Options::default());
    let pack = frames::pack(world.cache_bytes, &[3_000, 3_000]);
    let (a, b) = (frames::lookup_id(), frames::lookup_id());
    let frame = frames::commit(TRUSTED, &pack, &[Entry::new(a, 0), Entry::new(b, 1)]);
    let slot = world.reserve(&world.ci, "trusted", pack.bytes.len()).json()["packs"][0].clone();
    let pack_id = slot["pack_id"].as_str().expect("id");

    let early = world.commit(&world.ci, pack_id, &frame);
    assert_eq!(
        (early.status, early.error_code().as_str()),
        (409, "conflict")
    );
    assert_eq!(
        early.json()["error"]["reason"],
        "pack_incomplete",
        "the bytes have not arrived"
    );
    assert_eq!(world.put_slot(&slot, &pack.bytes).status, 200);

    let intruder = world.commit(&world.reader, pack_id, &frame);
    assert_eq!(intruder.status, 403);
    let other_writer = {
        let created = crate::support::fixture::token(
            &world.admin,
            "ci-2",
            &["read", "write:trusted"],
            &[&world.cache],
        );
        world
            .admin
            .with_token(created["token"].as_str().expect("token"))
    };
    assert_eq!(
        world.commit(&other_writer, pack_id, &frame).status,
        403,
        "only the uploader commits"
    );

    let first = world.commit(&world.ci, pack_id, &frame);
    assert_eq!(first.status, 200);
    let retry = world.commit(&world.ci, pack_id, &frame);
    assert_eq!(retry.status, 200);
    assert_eq!(
        retry.body, first.body,
        "a retry sees exactly the first answer"
    );
    let live = world.info(&world.ci)["quota"]["live_bytes"].clone();
    assert_eq!(live, 6_000, "a retry credits nothing twice");

    let different = frames::commit(TRUSTED, &pack, &[Entry::new(frames::lookup_id(), 0)]);
    let refused = world.commit(&world.ci, pack_id, &different);
    assert_eq!(
        (refused.status, refused.error_code().as_str()),
        (409, "conflict")
    );
    assert_eq!(refused.json()["error"]["reason"], "pack_closed");
}
