//! Size limits enforced while streaming, quotas and the per-tenant rate.

use crate::support::fixture::World;
use crate::support::frames::{self, Entry};
use crate::support::http::{connect, head_text, read_response, request};
use crate::support::server::Options;
use oxabl_cache_format as format;
use serde_json::json;
use std::io::Write;

#[test]
fn a_body_over_the_limit_is_refused_with_413_while_it_is_still_streaming() {
    let world = World::start("body-limits", Options::default());
    let mut stream = connect(&world.server.public);
    let headers = [
        ("Authorization", format!("Bearer {}", world.ci.token)),
        ("Content-Type", "application/json".to_owned()),
        ("Transfer-Encoding", "chunked".to_owned()),
    ];
    stream
        .write_all(head_text("POST", &world.path("/uploads"), &headers).as_bytes())
        .expect("head");
    let chunk = vec![b'a'; 64 * 1024];
    for _ in 0..17 {
        let framed = [format!("{:x}\r\n", chunk.len()).as_bytes(), &chunk, b"\r\n"].concat();
        if stream.write_all(&framed).is_err() {
            break;
        }
    }
    let refused = read_response(&mut stream);
    assert_eq!(
        refused.status, 413,
        "the response arrives before the body ends"
    );
    assert_eq!(refused.error_code(), "too_large");

    let declared = request(
        &world.server.public,
        "POST",
        &world.path("/lookup"),
        &[
            ("Authorization", format!("Bearer {}", world.ci.token)),
            ("Content-Type", "application/vnd.oxabl-cache.v1".to_owned()),
        ],
        &vec![0u8; 400_000],
    );
    assert_eq!(
        declared.status, 413,
        "a lookup frame has a size bound of its own"
    );
    let ids = vec![frames::lookup_id(); 10_001];
    let (oversized, _) = frames::lookup(&ids[..10_000], true, false, true, false);
    let mut one_more = oversized.clone();
    one_more.extend_from_slice(&ids[10_000]);
    assert_eq!(
        world
            .ci
            .binary("POST", &world.path("/lookup"), &one_more)
            .status,
        413,
        "10,001 ids do not fit the frame's size bound"
    );
    let too_many = {
        let mut frame = oversized;
        frame[8..12].copy_from_slice(&10_001u32.to_be_bytes());
        world.ci.binary("POST", &world.path("/lookup"), &frame)
    };
    assert_eq!(
        too_many.status, 400,
        "more than 10,000 ids is a malformed frame"
    );
}

#[test]
fn uploads_over_the_hard_quota_or_the_pack_limit_are_refused() {
    let world = World::build(
        "quota",
        Options::default(),
        json!({}),
        json!({"hard_bytes": 1_048_576}),
    );
    let reserved = world.reserve(&world.ci, "trusted", 2_000_000);
    assert_eq!(reserved.status, 200);
    assert_eq!(reserved.json()["packs"][0]["status"], "quota_exceeded");
    let fits = world.reserve(&world.ci, "trusted", 500_000);
    assert_eq!(fits.json()["packs"][0]["status"], "ok");
    let too_big = world.reserve(&world.ci, "trusted", 34 * 1024 * 1024);
    assert_eq!(
        (too_big.status, too_big.error_code().as_str()),
        (413, "too_large")
    );
    assert_eq!(world.info(&world.ci)["quota"]["pending_bytes"], 500_000);

    let pack = frames::pack(world.cache_bytes, &[400_000]);
    let (_, statuses) = world.publish(
        &world.ci,
        format::Namespace::Trusted,
        &pack,
        &[Entry::new(frames::lookup_id(), 0)],
    );
    assert_eq!(statuses, vec![format::CommitStatus::Committed]);
}

#[test]
fn api_routes_are_rate_limited_per_tenant_and_object_routes_are_not() {
    let world = World::build(
        "rate-limit",
        Options::default(),
        json!({"rate": 1, "burst": 60}),
        json!({}),
    );
    let pack = frames::pack(world.cache_bytes, &[3_000]);
    let key = frames::lookup_id();
    world.publish(
        &world.ci,
        format::Namespace::Trusted,
        &pack,
        &[Entry::new(key, 0)],
    );
    let (bytes, shape) = world.fetch(&world.reader, &[key]);
    let url = format::LookupResponse::decode(&bytes, &shape)
        .expect("OXLR")
        .packs[0]
        .url
        .to_owned();

    let mut limited = None;
    for _ in 0..200 {
        let answer = world.reader.json("GET", &world.path(""), None);
        if answer.status == 429 {
            limited = Some(answer);
            break;
        }
    }
    let limited = limited.expect("the burst runs out");
    assert_eq!(limited.error_code(), "rate_limited");
    assert!(limited.header("retry-after").is_some());
    assert_eq!(limited.json()["error"]["retryable"], true);
    for _ in 0..30 {
        assert_eq!(
            world.get_object(&url, None).status,
            200,
            "object routes are not rate limited"
        );
    }
    assert_eq!(
        world.reader.json("GET", &world.path(""), None).status,
        429,
        "the API is still throttled"
    );
}

#[test]
fn fetch_lookups_are_refused_over_the_monthly_egress_quota_and_plan_lookups_keep_working() {
    let world = World::start("egress", Options::default());
    let pack = frames::pack(world.cache_bytes, &[3_000]);
    let key = frames::lookup_id();
    world.publish(
        &world.ci,
        format::Namespace::Trusted,
        &pack,
        &[Entry::new(key, 0)],
    );
    let quota = world.server.admin_call(
        "PUT",
        &format!("/admin/v1/tenants/{}/quota", world.tenant),
        Some(&json!({"egress_bytes": 1_000, "rate": 50, "burst": 200})),
    );
    assert_eq!(quota.status, 200);

    let (bytes, shape) = world.fetch(&world.reader, &[key]);
    assert_eq!(
        format::LookupResponse::decode(&bytes, &shape)
            .expect("OXLR")
            .hits
            .len(),
        1
    );
    let month = "2000-01";
    let flushed = world
        .server
        .admin_call("GET", &format!("/admin/v1/usage?month={month}"), None);
    assert_eq!(
        flushed.status, 200,
        "reading usage flushes this node's counters"
    );

    let (frame, _) = frames::lookup(&[key], true, true, true, false);
    let refused = world.reader.binary("POST", &world.path("/lookup"), &frame);
    assert_eq!(
        (refused.status, refused.error_code().as_str()),
        (403, "quota_exceeded")
    );
    let (frame, shape) = frames::lookup(&[key], false, false, true, false);
    let plan = world.reader.binary("POST", &world.path("/lookup"), &frame);
    assert_eq!(plan.status, 200, "plan mode keeps working");
    assert_eq!(
        format::LookupResponse::decode(&plan.body, &shape)
            .expect("OXLR")
            .hits
            .len(),
        1
    );
}
