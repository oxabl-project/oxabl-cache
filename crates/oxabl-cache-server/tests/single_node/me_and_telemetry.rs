//! `GET /v1/me` and the sealed telemetry log of a cache.

use crate::support::fixture::{World, token};
use crate::support::frames;
use crate::support::http::request;
use crate::support::party::Party;
use crate::support::server::{Client, Options};
use base64::Engine as _;
use serde_json::Value;

fn me(client: &Client) -> Value {
    let answered = client.json("GET", "/v1/me", None);
    assert_eq!(
        answered.status,
        200,
        "{}",
        String::from_utf8_lossy(&answered.body)
    );
    answered.json()
}

fn names(me: &Value) -> Vec<String> {
    me["caches"]
        .as_array()
        .expect("caches")
        .iter()
        .map(|cache| cache["display_name"].as_str().expect("name").to_owned())
        .collect()
}

fn send(world: &World, client: &Client, body: &[u8]) -> crate::support::http::Response {
    client.binary("POST", &world.path("/telemetry"), body)
}

fn read(world: &World, client: &Client, query: &str) -> Value {
    let answered = client.json("GET", &world.path(&format!("/telemetry{query}")), None);
    assert_eq!(
        answered.status,
        200,
        "{}",
        String::from_utf8_lossy(&answered.body)
    );
    answered.json()
}

fn bodies(page: &Value) -> Vec<Vec<u8>> {
    page["reports"]
        .as_array()
        .expect("reports")
        .iter()
        .map(|report| {
            base64::engine::general_purpose::STANDARD
                .decode(report["body"].as_str().expect("body"))
                .expect("base64")
        })
        .collect()
}

#[test]
fn me_lists_exactly_the_caches_a_credential_holds_grants_on() {
    let world = World::start("me-grants", Options::default());
    let second = Party::on(&world.tenant, &world.admin, "second");
    let hidden = Party::on(&world.tenant, &world.admin, "hidden");
    let stranger = Party::in_new_tenant(&world, "stranger");
    second.publish(&world, 2);
    let both = token(
        &world.admin,
        "both",
        &["read", "write:personal"],
        &[&world.cache, &second.cache],
    );
    let both = world
        .admin
        .with_token(both["token"].as_str().expect("token"));

    let seen = me(&both);
    assert_eq!(seen["tenant"], world.tenant.as_str());
    assert_eq!(seen["tenant_admin"], false);
    assert_eq!(names(&seen), ["codebase", "second"]);
    let caches = seen["caches"].as_array().expect("caches");
    assert_eq!(caches[0]["id"], world.cache.as_str());
    assert_eq!(
        caches[0]["scopes"],
        serde_json::json!(["read", "write:personal"])
    );
    assert!(
        caches[1]["quota"]["live_bytes"].as_u64().expect("live") > 0,
        "the second cache holds the published pack"
    );
    assert_eq!(caches[0]["quota"]["live_bytes"], 0);
    assert!(caches.iter().all(|c| c["id"] != hidden.cache.as_str()));

    assert_eq!(names(&me(&world.reader)), ["codebase"]);
    let admin = me(&world.admin);
    assert_eq!(admin["tenant_admin"], true);
    assert_eq!(names(&admin), ["codebase", "hidden", "second"]);
    assert_eq!(admin["caches"][1]["scopes"], serde_json::json!([]));
    assert_eq!(names(&me(&stranger.ci)), ["codebase"]);
    assert_eq!(me(&stranger.ci)["caches"][0]["id"], stranger.cache.as_str());

    let anonymous = request(&world.server.public, "GET", "/v1/me", &[], b"");
    assert_eq!(anonymous.status, 401);
}

#[test]
fn members_append_sealed_reports_and_read_them_back_in_order() {
    let world = World::start("telemetry-log", Options::default());
    let sent: Vec<Vec<u8>> = (0..5).map(|n| frames::telemetry(1, 60 + n)).collect();
    for (client, body) in [&world.ci, &world.dev, &world.reader, &world.ci, &world.dev]
        .into_iter()
        .zip(&sent)
    {
        let stored = send(&world, client, body);
        assert_eq!(
            stored.status,
            201,
            "{}",
            String::from_utf8_lossy(&stored.body)
        );
    }

    let all = read(&world, &world.reader, "");
    assert_eq!(bodies(&all), sent, "byte for byte, in the order received");
    assert_eq!(all["next"], 5);
    let first = read(&world, &world.dev, "?limit=2");
    assert_eq!(bodies(&first), sent[..2]);
    let rest = read(
        &world,
        &world.dev,
        &format!("?after={}", first["next"].as_u64().expect("next")),
    );
    assert_eq!(bodies(&rest), sent[2..]);
    let none = read(&world, &world.dev, "?after=5");
    assert!(bodies(&none).is_empty());
    assert_eq!(none["next"], 5, "an empty page keeps the cursor");

    for query in ["?limit=0", "?limit=1001", "?after=-1", "?colour=blue"] {
        let refused = world
            .dev
            .json("GET", &world.path(&format!("/telemetry{query}")), None);
        assert_eq!(refused.status, 400, "{query}");
    }
}

#[test]
fn only_telemetry_frames_within_the_size_limit_are_stored() {
    let world = World::start("telemetry-shape", Options::default());
    let mut wrong_magic = frames::telemetry(1, 60);
    wrong_magic[..4].copy_from_slice(b"OXDP");
    let mut wrong_version = frames::telemetry(1, 60);
    wrong_version[4] = 2;
    for (what, body) in [
        ("wrong magic", wrong_magic),
        ("wrong version", wrong_version),
        ("only a tag", frames::telemetry(1, 16)),
        ("empty", Vec::new()),
    ] {
        let refused = send(&world, &world.ci, &body);
        assert_eq!(refused.status, 400, "{what}");
        assert_eq!(refused.error_code(), "bad_request", "{what}");
    }
    assert_eq!(
        send(&world, &world.ci, &frames::telemetry(1, 1_024 - 22)).status,
        201,
        "1,024 bytes is the largest report"
    );
    assert_eq!(
        send(&world, &world.ci, &frames::telemetry(1, 1_024 - 21)).status,
        413
    );
    let json = world.ci.json(
        "POST",
        &world.path("/telemetry"),
        Some(&serde_json::json!({"hits": 1})),
    );
    assert_eq!(json.status, 400, "{}", String::from_utf8_lossy(&json.body));
    assert_eq!(bodies(&read(&world, &world.ci, "")).len(), 1);
}

#[test]
fn telemetry_needs_a_credential_that_can_read_the_cache() {
    let world = World::start("telemetry-access", Options::default());
    let other = Party::on(&world.tenant, &world.admin, "other");
    let stranger = Party::in_new_tenant(&world, "stranger");
    let body = frames::telemetry(1, 60);
    assert_eq!(send(&world, &world.ci, &body).status, 201);

    let path = world.path("/telemetry");
    assert_eq!(
        request(&world.server.public, "GET", &path, &[], b"").status,
        401
    );
    assert_eq!(other.ci.json("GET", &path, None).status, 403);
    assert_eq!(other.ci.binary("POST", &path, &body).status, 403);
    assert_eq!(stranger.ci.json("GET", &path, None).status, 404);
    assert_eq!(stranger.ci.binary("POST", &path, &body).status, 404);
    assert_eq!(bodies(&read(&world, &world.reader, "")).len(), 1);
}

#[test]
fn a_cache_keeps_only_its_newest_reports() {
    let world = World::build(
        "telemetry-ring",
        Options::default(),
        serde_json::json!({"rate": 100_000, "burst": 10_000}),
        serde_json::json!({}),
    );
    let other = Party::on(&world.tenant, &world.admin, "other");
    let kept = 4_096;
    let mut last = Vec::new();
    for n in 0..kept + 3 {
        let body = frames::telemetry(1, 60);
        assert_eq!(send(&world, &world.ci, &body).status, 201, "report {n}");
        last.push(body);
    }
    assert_eq!(
        other
            .ci
            .binary("POST", &other.path("/telemetry"), &frames::telemetry(1, 60))
            .status,
        201
    );
    let mut seen = Vec::new();
    let mut after = 0;
    loop {
        let page = read(&world, &world.ci, &format!("?after={after}"));
        let got = bodies(&page);
        if got.is_empty() {
            break;
        }
        seen.extend(got);
        after = page["next"].as_u64().expect("next");
    }
    assert_eq!(seen.len(), kept, "the oldest three are dropped");
    assert_eq!(seen, last[3..], "the newest ones stay, in order");
    let other_page = other.ci.json("GET", &other.path("/telemetry"), None).json();
    assert_eq!(
        bodies(&other_page).len(),
        1,
        "another cache's log is its own"
    );
}
