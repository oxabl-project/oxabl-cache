//! What the server writes to stderr and exposes as metrics.

use crate::support::fixture::World;
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use oxabl_cache_format as format;
use serde_json::Value;

#[test]
fn logs_never_carry_tokens_signatures_query_strings_or_lookup_ids() {
    let world = World::start("log-canary", Options::default());
    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let key = frames::lookup_id();
    let reserved = world.reserve(&world.ci, "trusted", pack.bytes.len()).json()["packs"][0].clone();
    let url = reserved["url"].as_str().expect("url").to_owned();
    assert_eq!(world.put_object(&url, &pack.bytes).status, 200);
    let frame = frames::commit(format::Namespace::Trusted, &pack, &[Entry::new(key, 0)]);
    assert_eq!(
        world
            .commit(&world.ci, reserved["pack_id"].as_str().expect("id"), &frame)
            .status,
        200
    );
    let (bytes, shape) = world.fetch(&world.reader, &[key]);
    let fetched = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
    let get_url = fetched.packs[0].url.to_owned();
    assert_eq!(world.get_object(&get_url, Some("bytes=0-9")).status, 206);
    let _ =
        world
            .reader
            .with_token("oxc_wrongwrongwrongw_wrong")
            .json("GET", &world.path(""), None);

    let log = world.server.stderr();
    let canaries = [
        ("the ci token", world.ci.token.clone()),
        ("the reader token", world.reader.token.clone()),
        ("the admin token", world.admin.token.clone()),
        ("the operator token", world.server.operator.clone()),
        (
            "the secret of a token",
            world
                .ci
                .token
                .rsplit('_')
                .next()
                .expect("secret")
                .to_owned(),
        ),
        (
            "a signed PUT query",
            url.split('?').nth(1).expect("query").to_owned(),
        ),
        (
            "a signature",
            url.split("sig=").nth(1).expect("sig").to_owned(),
        ),
        (
            "a signed GET query",
            get_url.split('?').nth(1).expect("query").to_owned(),
        ),
        (
            "the lookup id of the stored entry",
            key.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        ),
        ("a bearer header", "Bearer".to_owned()),
        ("a query separator", "?exp=".to_owned()),
        (
            "the pack id in a path",
            format!("/objects/{}", reserved["pack_id"].as_str().expect("id")),
        ),
    ];
    for (what, text) in canaries {
        assert!(!log.contains(&text), "{what} appears in the log");
    }

    let lines: Vec<Value> = log
        .lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|_| panic!("a log line is JSON: {line}"))
        })
        .collect();
    let requests: Vec<&Value> = lines
        .iter()
        .filter(|line| line["event"] == "request")
        .collect();
    assert!(requests.len() >= 8, "the requests are logged");
    let object_put = requests
        .iter()
        .find(|line| {
            line["route"] == "/v1/caches/{cache}/objects/{pack}" && line["method"] == "PUT"
        })
        .expect("the object PUT is logged by its route template");
    assert!(object_put["request_id"].is_string() && object_put["status"] == 200);
    assert!(object_put["duration_ms"].is_number());
    let lookup = requests
        .iter()
        .find(|line| line["route"] == "/v1/caches/{cache}/lookup")
        .expect("lookup is logged");
    assert!(
        lookup["tenant"].is_string()
            && lookup["cache"].is_string()
            && lookup["principal"].is_string()
    );
    assert_eq!(lookup["ids"], 1);
}

#[test]
fn the_metrics_endpoint_exposes_request_lookup_and_commit_series() {
    let world = World::start("metrics", Options::default());
    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let key = frames::lookup_id();
    world.publish(
        &world.ci,
        format::Namespace::Trusted,
        &pack,
        &[Entry::new(key, 0)],
    );
    world.fetch(&world.reader, &[key, frames::lookup_id()]);
    std::thread::sleep(std::time::Duration::from_millis(600));

    let page = world.server.metrics_text();
    for series in [
        "oxabl_cache_http_requests_total{route=\"/v1/caches/{cache}/lookup\",status=\"200\"} 1",
        "oxabl_cache_lookup_ids_total{result=\"hit\"} 1",
        "oxabl_cache_lookup_ids_total{result=\"miss\"} 1",
        "oxabl_cache_commit_entries_total{status=\"committed\"} 1",
        "oxabl_cache_gc_leader 1",
        "# TYPE oxabl_cache_http_request_seconds histogram",
        "oxabl_cache_metadata_query_seconds_count",
        "oxabl_cache_pack_bytes{state=\"live\"} 4000",
    ] {
        assert!(page.contains(series), "missing `{series}` in:\n{page}");
    }
}
