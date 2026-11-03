//! Deleting caches and tenants: a tombstone at once, removal by the GC leader in batches.

use crate::support::fixture::{World, wait_until};
use crate::support::frames;
use crate::support::objects::Kind;
use crate::support::party::Party;
use crate::support::rows::{count, rows_of};
use crate::support::server::Options;
use oxabl_cache_format as format;
use serde_json::json;
use std::time::{Duration, Instant};

const CACHE_TABLES: [&str; 5] = [
    "entries",
    "entry_access",
    "segments",
    "packs",
    "cache_owner_keys",
];
const WAIT: Duration = Duration::from_secs(60);

/// Short grace, long leases and URLs: a slow machine must not expire what a test still holds.
fn options(grace: &'static str) -> Options {
    Options {
        grace,
        lease: "60s",
        url_lifetime: "60s",
        ..Options::default()
    }
}

fn rows_left(world: &World, party: &Party) -> i64 {
    CACHE_TABLES
        .iter()
        .map(|table| rows_of(&world.server, table, "cache_id", &party.cache_bytes))
        .sum::<i64>()
        + rows_of(&world.server, "caches", "cache_id", &party.cache_bytes)
}

fn objects_left(world: &World, party: &Party, packs: &[String]) -> usize {
    packs
        .iter()
        .filter(|pack| world.server.objects.exists(&party.cache, pack))
        .count()
}

fn get_url(party: &Party, keys: &[[u8; 32]]) -> String {
    let (frame, shape) = frames::lookup(keys, true, true, true, false);
    let answered = party.ci.binary("POST", &party.path("/lookup"), &frame);
    assert_eq!(answered.status, 200);
    format::LookupResponse::decode(&answered.body, &shape)
        .expect("OXLR")
        .packs[0]
        .url
        .to_owned()
}

fn create_named(admin: &crate::support::server::Client, name: &str) -> u16 {
    admin
        .json(
            "POST",
            "/v1/tenant/caches",
            Some(&json!({
                "display_name": name,
                "owner_keys": [crate::support::fixture::owner_key()],
            })),
        )
        .status
}

#[test]
fn a_deleted_cache_is_404_at_once_keeps_its_name_until_teardown_and_leaves_everything_else() {
    let world = World::start("delete-cache", options("6s"));
    let doomed = Party::on(&world.tenant, &world.admin, "doomed");
    let sibling = Party::on(&world.tenant, &world.admin, "sibling");
    let stranger = Party::in_new_tenant(&world, "stranger");
    let (first, keys) = doomed.publish(&world, 3);
    let (second, more) = doomed.publish(&world, 2);
    let packs = [first, second];
    let (sibling_pack, sibling_keys) = sibling.publish(&world, 3);
    let (stranger_pack, stranger_keys) = stranger.publish(&world, 3);
    let url = (crate::STORE == Kind::Fs).then(|| get_url(&doomed, &keys));
    assert_eq!(doomed.hits(&keys), Ok(3));
    let sibling_rows = rows_left(&world, &sibling);
    let stranger_rows = rows_left(&world, &stranger);

    let deleted = world.admin.json(
        "DELETE",
        &format!("/v1/tenant/caches/{}", doomed.cache),
        None,
    );
    assert_eq!(deleted.status, 202);
    assert!(deleted.body.is_empty());

    assert_eq!(doomed.hits(&keys), Err(404), "a lookup finds nothing");
    assert_eq!(doomed.hits(&more), Err(404));
    assert_eq!(doomed.ci.json("GET", &doomed.path(""), None).status, 404);
    let pack = frames::pack(doomed.cache_bytes, &[3_000]);
    assert_eq!(
        world
            .reserve_bytes_in(&doomed.ci, &doomed.cache, "trusted", &pack.bytes)
            .status,
        404,
        "no pack is reserved"
    );
    if let Some(url) = &url {
        assert_eq!(
            world.get_object(url, None).status,
            404,
            "its bytes are not served"
        );
    }
    let again = world.admin.json(
        "DELETE",
        &format!("/v1/tenant/caches/{}", doomed.cache),
        None,
    );
    assert_eq!(again.status, 404, "a second delete finds nothing");
    let foreign = stranger.admin.json(
        "DELETE",
        &format!("/v1/tenant/caches/{}", sibling.cache),
        None,
    );
    assert_eq!(foreign.status, 404, "another tenant's cache is not found");
    let listed = world.admin.json("GET", "/v1/tenant/caches", None).json();
    let names: Vec<&str> = listed["caches"]
        .as_array()
        .expect("caches")
        .iter()
        .map(|cache| cache["display_name"].as_str().expect("name"))
        .collect();
    assert!(!names.contains(&"doomed"), "{names:?}");
    assert_eq!(
        create_named(&world.admin, "doomed"),
        409,
        "the name stays taken"
    );
    assert_eq!(
        objects_left(&world, &doomed, &packs),
        2,
        "bytes wait for the grace"
    );
    assert!(rows_left(&world, &doomed) > 0, "rows wait for the grace");
    assert_eq!(sibling.hits(&sibling_keys), Ok(3));
    assert_eq!(stranger.hits(&stranger_keys), Ok(3));

    assert!(
        wait_until(WAIT, || objects_left(&world, &doomed, &packs) == 0
            && rows_left(&world, &doomed) == 0),
        "the leader removes the bytes and then the rows"
    );
    assert_eq!(
        create_named(&world.admin, "doomed"),
        201,
        "the name is free once the teardown is complete"
    );
    for (table, column) in [
        ("token_grants", "cache_id"),
        ("grants", "cache_id"),
        ("scrub_queue", "cache_id"),
    ] {
        assert_eq!(
            rows_of(&world.server, table, column, &doomed.cache_bytes),
            0,
            "{table}"
        );
    }
    assert_eq!(
        rows_left(&world, &sibling),
        sibling_rows,
        "a sibling cache is untouched"
    );
    assert_eq!(
        rows_left(&world, &stranger),
        stranger_rows,
        "another tenant is untouched"
    );
    assert_eq!(sibling.hits(&sibling_keys), Ok(3));
    assert_eq!(stranger.hits(&stranger_keys), Ok(3));
    assert!(world.server.objects.exists(&sibling.cache, &sibling_pack));
    assert!(world.server.objects.exists(&stranger.cache, &stranger_pack));

    assert_eq!(
        world
            .server
            .admin_call("GET", "/admin/v1/usage?month=2000-01", None)
            .status,
        200,
        "reading usage flushes the counters"
    );
    assert!(
        rows_of(
            &world.server,
            "usage_ledger",
            "cache_id",
            &doomed.cache_bytes
        ) > 0,
        "the usage of the deleted cache stays in the ledger"
    );
}

#[test]
fn a_deleted_tenant_loses_its_credentials_at_once_and_its_bytes_and_rows_after_the_grace() {
    let world = World::start("delete-tenant", options("6s"));
    let stranger = Party::in_new_tenant(&world, "stranger");
    let second = Party::on(&stranger.tenant, &stranger.admin, "second");
    let (first_pack, keys) = stranger.publish(&world, 3);
    let (second_pack, second_keys) = second.publish(&world, 2);
    let (own_pack, own_keys) = {
        let party = Party::on(&world.tenant, &world.admin, "own");
        let (pack, keys) = party.publish(&world, 2);
        (pack, (party, keys))
    };
    let (own, own_ids) = own_keys;
    assert_eq!(stranger.hits(&keys), Ok(3));
    let tenant_path = format!("/admin/v1/tenants/{}", stranger.tenant);

    let deleted = world.server.admin_call("DELETE", &tenant_path, None);
    assert_eq!(deleted.status, 202);
    assert!(deleted.body.is_empty());

    assert_eq!(stranger.hits(&keys), Err(401), "its tokens stop working");
    assert_eq!(second.hits(&second_keys), Err(401));
    assert_eq!(
        stranger.admin.json("GET", "/v1/tenant/caches", None).status,
        401
    );
    let listed = world
        .server
        .admin_call("GET", "/admin/v1/tenants", None)
        .json();
    assert_eq!(listed["tenants"].as_array().expect("tenants").len(), 1);
    assert_eq!(
        world.server.admin_call("DELETE", &tenant_path, None).status,
        404
    );
    assert_eq!(
        world
            .server
            .admin_call("PATCH", &tenant_path, Some(&json!({"status": "active"})))
            .status,
        404,
        "a deleted tenant cannot be resumed"
    );
    assert_eq!(
        world
            .server
            .admin_call(
                "POST",
                &format!("{tenant_path}/tokens"),
                Some(&json!({"name": "late"}))
            )
            .status,
        404
    );
    let reused = world.server.admin_call(
        "POST",
        "/admin/v1/tenants",
        Some(&json!({"name": "stranger"})),
    );
    assert_eq!(reused.status, 409, "the name stays taken");
    assert_eq!(own.hits(&own_ids), Ok(2), "another tenant is untouched");

    let tenant_bytes = stranger
        .admin
        .json("GET", "/v1/tenant/usage?month=2000-01", None);
    assert_eq!(tenant_bytes.status, 401);
    assert!(
        wait_until(WAIT, || {
            let tenant = &stranger.tenant;
            let rows = |sql: &str| count(&world.server, sql);
            let id = tenant_id_blob(tenant);
            objects_left(&world, &stranger, std::slice::from_ref(&first_pack)) == 0
                && objects_left(&world, &second, std::slice::from_ref(&second_pack)) == 0
                && rows(&format!(
                    "SELECT count(*) FROM tenants WHERE tenant_id = {id}"
                )) == 0
                && rows(&format!(
                    "SELECT count(*) FROM caches WHERE tenant_id = {id}"
                )) == 0
                && rows(&format!(
                    "SELECT count(*) FROM tokens WHERE tenant_id = {id}"
                )) == 0
        }),
        "the leader removes the tenant's bytes and rows"
    );
    assert_eq!(rows_left(&world, &stranger), 0);
    assert_eq!(rows_left(&world, &second), 0);
    let reborn = world.server.admin_call(
        "POST",
        "/admin/v1/tenants",
        Some(&json!({"name": "stranger"})),
    );
    assert_eq!(reborn.status, 201, "the name is free again");
    assert_eq!(own.hits(&own_ids), Ok(2));
    assert!(world.server.objects.exists(&own.cache, &own_pack));
    assert_eq!(
        world
            .server
            .admin_call("GET", "/admin/v1/usage?month=2000-01", None)
            .status,
        200
    );
    assert!(
        count(
            &world.server,
            &format!(
                "SELECT count(*) FROM usage_ledger WHERE tenant_id = {}",
                tenant_id_blob(&stranger.tenant)
            )
        ) > 0,
        "the usage of the deleted tenant stays in the ledger"
    );
}

fn tenant_id_blob(tenant: &str) -> String {
    crate::support::rows::blob(&format::TenantId::from_text(tenant).expect("tenant id").0)
}

#[test]
fn a_crash_in_the_middle_of_a_teardown_resumes_where_it_stopped() {
    let mut world = World::start(
        "delete-resume",
        Options {
            extra: "batch = 2",
            ..options("2s")
        },
    );
    let doomed = Party::on(&world.tenant, &world.admin, "doomed");
    let mut sibling = Party::on(&world.tenant, &world.admin, "sibling");
    let mut packs = Vec::new();
    for _ in 0..8 {
        packs.push(doomed.publish(&world, 2).0);
    }
    let (_, sibling_keys) = sibling.publish(&world, 3);
    let sibling_rows = rows_left(&world, &sibling);
    let total = packs.len();
    let entries = rows_of(
        &world.server,
        "entry_access",
        "cache_id",
        &doomed.cache_bytes,
    );
    assert_eq!(entries, 16);

    let deleted = world.admin.json(
        "DELETE",
        &format!("/v1/tenant/caches/{}", doomed.cache),
        None,
    );
    assert_eq!(deleted.status, 202);

    let midway = |reached: &dyn Fn() -> bool| {
        let started = Instant::now();
        while started.elapsed() < WAIT {
            if reached() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the teardown never reached the awaited point");
    };
    midway(&|| {
        let left = objects_left(&world, &doomed, &packs);
        left > 0 && left < total
    });
    world.server.kill();
    let left = objects_left(&world, &doomed, &packs);
    assert!(
        left > 0 && left < total,
        "killed while deleting objects: {left} of {total} left"
    );
    assert_eq!(
        rows_of(
            &world.server,
            "entry_access",
            "cache_id",
            &doomed.cache_bytes
        ),
        entries,
        "no row went before the last object"
    );
    world.server.restart();
    world.reconnect();

    midway(&|| {
        let rows = rows_of(
            &world.server,
            "entry_access",
            "cache_id",
            &doomed.cache_bytes,
        );
        objects_left(&world, &doomed, &packs) == 0 && rows > 0 && rows < entries
    });
    world.server.kill();
    assert_eq!(objects_left(&world, &doomed, &packs), 0);
    assert!(rows_left(&world, &doomed) > 0, "killed while deleting rows");
    world.server.restart();
    world.reconnect();

    assert!(
        wait_until(WAIT, || rows_left(&world, &doomed) == 0),
        "the restarted leader finishes the teardown"
    );
    assert_eq!(objects_left(&world, &doomed, &packs), 0);
    assert_eq!(create_named(&world.admin, "doomed"), 201);
    sibling.reconnect(&world);
    assert_eq!(rows_left(&world, &sibling), sibling_rows);
    assert_eq!(sibling.hits(&sibling_keys), Ok(3));
}
