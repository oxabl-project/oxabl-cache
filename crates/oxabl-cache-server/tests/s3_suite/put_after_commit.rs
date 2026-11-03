//! No PUT reaches a committed pack: the server issues no URL, and a stale URL is
//! refused by the store where it supports `If-None-Match`, caught by the ETag where not.

use crate::support::fixture::{World, hex};
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use oxabl_cache_format as format;
use serde_json::json;

const TRUSTED: format::Namespace = format::Namespace::Trusted;

fn reserve_ref(world: &World, client_ref: &[u8], size: usize) -> serde_json::Value {
    let reserved = world.ci.json(
        "POST",
        &world.path("/uploads"),
        Some(&json!({
            "namespace": "trusted",
            "packs": [{"client_ref": hex(client_ref), "size": size}],
        })),
    );
    assert_eq!(reserved.status, 200);
    reserved.json()["packs"][0].clone()
}

#[test]
fn a_put_after_commit_is_refused_by_url_issuance_and_by_the_store_or_caught_by_the_etag() {
    let world = World::start("s3-put-after-commit", Options::default());
    let supports = world.info(&world.ci)["store"]["if_none_match"]
        .as_bool()
        .expect("flag");
    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let client_ref = frames::random(16);
    let slot = reserve_ref(&world, &client_ref, pack.bytes.len());
    let pack_id = slot["pack_id"].as_str().expect("id").to_owned();
    assert_eq!(world.put_slot(&slot, &pack.bytes).status, 200);
    let key = frames::lookup_id();
    let frame = frames::commit(TRUSTED, &pack, &[Entry::new(key, 0)]);
    assert_eq!(world.commit(&world.ci, &pack_id, &frame).status, 200);

    let again = reserve_ref(&world, &client_ref, pack.bytes.len());
    assert_eq!(again["status"], "committed");
    assert!(
        again.get("url").is_none(),
        "no URL is issued for a committed pack"
    );

    let replacement = frames::random(pack.bytes.len());
    let replay = world.put_slot(&slot, &replacement);
    if supports {
        assert_eq!(replay.status, 412, "the store refuses a second write");
        assert_eq!(world.read_object(&pack_id), pack.bytes);
        assert_eq!(world.commit(&world.ci, &pack_id, &frame).status, 200);
    } else {
        eprintln!(
            "{:?} has no If-None-Match: the stale URL overwrites and the ETag catches it",
            crate::STORE
        );
        assert_eq!(replay.status, 200);
        let repeat = world.commit(&world.ci, &pack_id, &frame);
        assert_eq!(
            (repeat.status, repeat.json()["error"]["reason"].as_str()),
            (409, Some("pack_changed"))
        );
        let scrub = world.server.admin_call(
            "POST",
            &format!("/admin/v1/caches/{}/scrub", world.cache),
            None,
        );
        assert_eq!(scrub.json()["corrupt_segments"], 1);
        let (bytes, shape) = world.fetch(&world.reader, &[key]);
        let found = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
        assert!(found.hits.is_empty(), "the replaced pack serves nothing");
    }
}
