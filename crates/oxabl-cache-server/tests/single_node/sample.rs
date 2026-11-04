//! `GET /v1/caches/{cache}/sample`: random live ids of the trusted namespace.

use crate::support::fixture::{World, hex};
use crate::support::frames::{self, Entry};
use crate::support::http::request;
use crate::support::party::Party;
use crate::support::server::Options;
use oxabl_cache_format as format;
use std::collections::BTreeSet;

const TRUSTED: format::Namespace = format::Namespace::Trusted;

fn publish(world: &World, ns: format::Namespace, count: usize) -> Vec<[u8; 32]> {
    let client = match ns {
        format::Namespace::Trusted => &world.ci,
        format::Namespace::Personal => &world.dev,
    };
    let pack = frames::pack(world.cache_bytes, &vec![3_000; count]);
    let ids: Vec<[u8; 32]> = (0..count).map(|_| frames::lookup_id()).collect();
    let entries: Vec<Entry> = ids
        .iter()
        .enumerate()
        .map(|(index, id)| Entry::new(*id, index as u32))
        .collect();
    let (_, statuses) = world.publish(client, ns, &pack, &entries);
    assert_eq!(statuses, vec![format::CommitStatus::Committed; count]);
    ids
}

fn sample(world: &World, query: &str) -> crate::support::http::Response {
    world
        .reader
        .json("GET", &world.path(&format!("/sample?{query}")), None)
}

fn sampled(world: &World, n: usize) -> Vec<String> {
    let answered = sample(world, &format!("namespace=trusted&n={n}"));
    assert_eq!(answered.status, 200);
    answered.json()["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|id| id.as_str().expect("hex id").to_owned())
        .collect()
}

#[test]
fn an_empty_cache_has_an_empty_sample() {
    let world = World::start("sample-empty", Options::default());
    assert!(sampled(&world, 5).is_empty());
}

#[test]
fn a_sample_holds_distinct_live_trusted_ids_only_and_varies() {
    let world = World::start("sample-ids", Options::default());
    let trusted: BTreeSet<String> = publish(&world, TRUSTED, 150)
        .iter()
        .map(|id| hex(id))
        .collect();
    let personal: BTreeSet<String> = publish(&world, format::Namespace::Personal, 20)
        .iter()
        .map(|id| hex(id))
        .collect();

    let some = sampled(&world, 7);
    assert_eq!(some.len(), 7);
    assert_eq!(some.iter().collect::<BTreeSet<_>>().len(), 7, "distinct");
    assert!(
        some.iter().all(|id| trusted.contains(id)),
        "live trusted ids"
    );

    let all = sampled(&world, 1_000);
    assert_eq!(
        all.iter().cloned().collect::<BTreeSet<_>>(),
        trusted,
        "asking for more than the cache holds returns all of it"
    );
    assert!(all.iter().all(|id| !personal.contains(id)));

    let draws: BTreeSet<Vec<String>> = (0..10).map(|_| sampled(&world, 5)).collect();
    assert!(draws.len() > 1, "ten samples are not all the same ids");
}

#[test]
fn the_count_is_bounded_and_the_namespace_is_trusted() {
    let world = World::start("sample-bounds", Options::default());
    publish(&world, TRUSTED, 3);
    assert_eq!(
        sampled(&world, 1_000).len(),
        3,
        "1,000 is the largest count"
    );
    for query in [
        "namespace=trusted&n=0",
        "namespace=trusted&n=1001",
        "namespace=trusted&n=-1",
        "namespace=trusted&n=many",
        "namespace=trusted",
        "n=5",
        "namespace=personal&n=5",
        "namespace=other&n=5",
    ] {
        let refused = sample(&world, query);
        assert_eq!(refused.status, 400, "{query}");
        assert_eq!(refused.error_code(), "bad_request", "{query}");
    }
}

#[test]
fn a_sample_needs_a_credential_that_can_read_the_cache() {
    let world = World::start("sample-access", Options::default());
    publish(&world, TRUSTED, 3);
    let path = world.path("/sample?namespace=trusted&n=3");
    let anonymous = request(&world.server.public, "GET", &path, &[], b"");
    assert_eq!(anonymous.status, 401);

    let stranger = Party::in_new_tenant(&world, "stranger");
    let across = stranger.ci.json("GET", &path, None);
    assert_eq!(across.status, 404, "another tenant's cache is not found");
    assert_eq!(world.dev.json("GET", &path, None).status, 200);
}
