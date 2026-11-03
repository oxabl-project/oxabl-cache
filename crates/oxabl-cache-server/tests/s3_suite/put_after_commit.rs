//! No PUT reaches a committed pack: the server issues no URL, and a stale URL is
//! refused by the store where it supports `If-None-Match`, and by the signed digest where not.

use crate::support::fixture::{World, hex};
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use base64::Engine as _;
use oxabl_cache_format as format;
use serde_json::json;
use sha2::{Digest, Sha256};

const TRUSTED: format::Namespace = format::Namespace::Trusted;

fn reserve_ref(world: &World, client_ref: &[u8], bytes: &[u8]) -> serde_json::Value {
    let reserved = world.ci.json(
        "POST",
        &world.path("/uploads"),
        Some(&json!({
            "namespace": "trusted",
            "packs": [{
                "client_ref": hex(client_ref),
                "size": bytes.len(),
                "sha256": base64::engine::general_purpose::STANDARD.encode(Sha256::digest(bytes)),
            }],
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
    let slot = reserve_ref(&world, &client_ref, &pack.bytes);
    let pack_id = slot["pack_id"].as_str().expect("id").to_owned();
    assert_eq!(world.put_slot(&slot, &pack.bytes).status, 200);
    let key = frames::lookup_id();
    let frame = frames::commit(TRUSTED, &pack, &[Entry::new(key, 0)]);
    assert_eq!(world.commit(&world.ci, &pack_id, &frame).status, 200);

    let again = reserve_ref(&world, &client_ref, &pack.bytes);
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
        assert_eq!(
            world.info(&world.ci)["store"]["checksum_sha256"],
            true,
            "a store without If-None-Match must verify the digest signed into the URL"
        );
        assert!(
            replay.status >= 400,
            "the signed digest refuses other bytes, got {}",
            replay.status
        );
        assert_eq!(world.read_object(&pack_id), pack.bytes);
        assert_eq!(world.commit(&world.ci, &pack_id, &frame).status, 200);
    }
}
