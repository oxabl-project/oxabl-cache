//! The optional capabilities the probe found are reported and signed into PUT URLs.

use crate::support::fixture::{World, hex};
use crate::support::frames;
use crate::support::objects::Kind;
use crate::support::server::Options;
use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// What each test server is known to support: `(If-None-Match, x-amz-checksum-sha256)`.
fn known_capabilities() -> (bool, bool) {
    match crate::STORE {
        Kind::Garage => (false, true),
        Kind::Versitygw => (true, true),
        Kind::Fs => panic!("an S3 scenario ran against a local store"),
    }
}

fn reserve_with_digest(world: &World, size: usize, digest: &[u8]) -> Value {
    let reserved = world.ci.json(
        "POST",
        &world.path("/uploads"),
        Some(&json!({
            "namespace": "trusted",
            "packs": [{
                "client_ref": hex(&frames::random(16)),
                "size": size,
                "sha256": base64::engine::general_purpose::STANDARD.encode(digest),
            }],
        })),
    );
    assert_eq!(
        reserved.status,
        200,
        "{}",
        String::from_utf8_lossy(&reserved.body)
    );
    reserved.json()["packs"][0].clone()
}

#[test]
fn the_capabilities_the_probe_found_are_reported_and_signed_into_put_urls() {
    let world = World::start("s3-capabilities", Options::default());
    let store = world.info(&world.reader)["store"].clone();
    assert_eq!(store["kind"], "s3");
    let if_none_match = store["if_none_match"].as_bool().expect("flag");
    let checksum = store["checksum_sha256"].as_bool().expect("flag");
    eprintln!(
        "capabilities of {:?}: if_none_match={if_none_match} checksum_sha256={checksum}",
        crate::STORE
    );
    assert_eq!(
        (if_none_match, checksum),
        known_capabilities(),
        "the probe found other capabilities than this server version has; update the table if the server changed"
    );

    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let digest = Sha256::digest(&pack.bytes);
    let slot = reserve_with_digest(&world, pack.bytes.len(), &digest);
    let headers = slot["headers"].as_object().expect("headers");
    assert_eq!(
        headers.get("if-none-match").map(|value| value == "*"),
        if_none_match.then_some(true),
        "if-none-match is signed exactly when the store honours it"
    );
    assert_eq!(
        headers.contains_key("x-amz-checksum-sha256"),
        checksum,
        "the checksum is signed exactly when the store verifies it"
    );
    assert_eq!(world.put_slot(&slot, &pack.bytes).status, 200);
    assert_eq!(
        world.read_object(slot["pack_id"].as_str().expect("id")),
        pack.bytes
    );

    if checksum {
        let wrong = reserve_with_digest(&world, pack.bytes.len(), &Sha256::digest(b"other bytes"));
        let refused = world.put_slot(&wrong, &pack.bytes);
        assert!(
            refused.status >= 400,
            "a PUT whose bytes miss the signed digest is refused, got {}",
            refused.status
        );
        assert!(!world.object_exists(wrong["pack_id"].as_str().expect("id")));
    } else {
        eprintln!(
            "skipped: the digest-mismatch refusal, {:?} does not verify x-amz-checksum-sha256",
            crate::STORE
        );
    }
}
