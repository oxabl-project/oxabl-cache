//! A cache from nothing to a fetched, byte-identical segment.

use crate::support::fixture::World;
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use oxabl_cache_format as format;
use serde_json::json;

const TRUSTED: format::Namespace = format::Namespace::Trusted;

#[test]
fn an_operator_bootstraps_a_tenant_and_ci_uploads_then_readers_fetch_byte_identical_segments() {
    let world = World::start("round-trip", Options::default());
    let info = world.info(&world.reader);
    assert_eq!(info["epoch"], 0);
    assert_eq!(info["roster_version"], 0);
    assert_eq!(info["store"]["kind"], "fs");
    assert_eq!(info["limits"]["lookup_ids"], 10_000);

    let pack = frames::pack(world.cache_bytes, &[3_000, 5_000]);
    let (a, b, missing) = (
        frames::lookup_id(),
        frames::lookup_id(),
        frames::lookup_id(),
    );
    let entries = [Entry::new(a, 0), Entry::new(b, 1)];
    let (_, statuses) = world.publish(&world.ci, TRUSTED, &pack, &entries);
    assert_eq!(statuses, vec![format::CommitStatus::Committed; 2]);

    let (frame, shape) = frames::lookup(&[a, missing, b], false, false, true, false);
    let plan = world.reader.binary("POST", &world.path("/lookup"), &frame);
    assert_eq!(plan.status, 200);
    let plan = format::LookupResponse::decode(&plan.body, &shape).expect("OXLR");
    let found: Vec<u32> = plan.hits.iter().map(|hit| hit.index).collect();
    assert_eq!(found, vec![0, 2], "plan mode answers presence only");
    assert!(plan.packs.is_empty() && plan.hits.iter().all(|hit| hit.record.is_none()));

    let (bytes, shape) = world.fetch(&world.reader, &[a, b]);
    let fetched = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
    assert_eq!(fetched.hits.len(), 2);
    assert_eq!(fetched.packs.len(), 1);
    let url = fetched.packs[0].url.to_owned();
    assert!(!url.is_empty());
    for (hit, entry) in fetched.hits.iter().zip(&entries) {
        let located = hit.record.as_ref().expect("fetch mode carries records");
        assert_eq!(
            located.record,
            entry.record.as_slice(),
            "the sealed record is returned untouched"
        );
    }

    let second = fetched.hits[1].record.as_ref().expect("record");
    let (offset, length) = (second.offset, u64::from(second.length));
    let range = format!("bytes={offset}-{}", offset + length - 1);
    let ranged = world.get_object(&url, Some(&range));
    assert_eq!(ranged.status, 206);
    assert_eq!(
        ranged.body, pack.contents[1],
        "a ranged GET returns exactly the segment"
    );
    assert_eq!(
        ranged.header("content-range"),
        Some(
            format!(
                "bytes {offset}-{}/{}",
                offset + length - 1,
                pack.bytes.len()
            )
            .as_str()
        )
    );
    let whole = world.get_object(&url, None);
    assert_eq!((whole.status, whole.body), (200, pack.bytes.clone()));
    let outside = world.get_object(&url, Some(&format!("bytes={}-", pack.bytes.len())));
    assert_eq!(outside.status, 416);
    assert_eq!(outside.error_code(), "bad_request");

    let (frame, shape) = frames::lookup(&[a], true, false, false, true);
    let personal = world.dev.binary("POST", &world.path("/lookup"), &frame);
    let personal = format::LookupResponse::decode(&personal.body, &shape).expect("OXLR");
    assert!(
        personal.hits.is_empty(),
        "trusted entries are not in the personal namespace"
    );

    let info = world.info(&world.reader);
    assert_eq!(info["quota"]["live_bytes"], 8_000);
    assert_eq!(info["quota"]["pending_bytes"], 0);
    let urls = world.reader.json(
        "POST",
        &world.path("/urls"),
        Some(&json!({"packs": [fetched.packs[0].pack_id.to_hex()]})),
    );
    assert_eq!(urls.status, 200);
    assert_eq!(urls.json()["urls"].as_object().expect("urls").len(), 1);
}

#[test]
fn every_error_is_the_json_envelope_and_every_response_has_a_request_id() {
    let world = World::start("envelope", Options::default());
    let missing = world.reader.json("GET", "/v1/nothing/here", None);
    assert_eq!(missing.status, 404);
    assert_eq!(missing.error_code(), "not_found");
    let id = missing
        .header("x-request-id")
        .expect("request id")
        .to_owned();
    assert_eq!(id.len(), 26);
    assert_eq!(missing.json()["error"]["request_id"], id.as_str());

    let wrong_method = world.reader.json("DELETE", &world.path(""), None);
    assert_eq!(wrong_method.status, 405);
    assert_eq!(
        wrong_method.json()["error"]["request_id"]
            .as_str()
            .map(str::len),
        Some(26)
    );

    let supplied = crate::support::http::request(
        &world.server.public,
        "GET",
        &world.path(""),
        &[
            ("Authorization", format!("Bearer {}", world.reader.token)),
            ("X-Request-Id", "0123456789abcdefghjkmnpqrs".to_owned()),
        ],
        b"",
    );
    assert_eq!(
        supplied.header("x-request-id"),
        Some("0123456789abcdefghjkmnpqrs")
    );
    let invalid = crate::support::http::request(
        &world.server.public,
        "GET",
        &world.path(""),
        &[
            ("Authorization", format!("Bearer {}", world.reader.token)),
            ("X-Request-Id", "not a base32 id".to_owned()),
        ],
        b"",
    );
    assert_ne!(invalid.header("x-request-id"), Some("not a base32 id"));
    assert_eq!(invalid.header("x-request-id").map(str::len), Some(26));

    let bad_frame = world
        .reader
        .binary("POST", &world.path("/lookup"), b"not a frame");
    assert_eq!(bad_frame.status, 400);
    assert_eq!(bad_frame.error_code(), "bad_request");
    let (frame, _) = frames::lookup(&[frames::lookup_id()], false, false, true, false);
    let wrong_type = crate::support::http::request(
        &world.server.public,
        "POST",
        &world.path("/lookup"),
        &[
            ("Authorization", format!("Bearer {}", world.reader.token)),
            ("Content-Type", "application/json".to_owned()),
        ],
        &frame,
    );
    assert_eq!(
        wrong_type.status, 400,
        "a frame needs the binary content type"
    );
    assert_eq!(
        world
            .reader
            .binary("POST", &world.path("/lookup"), &frame)
            .status,
        200
    );
}
