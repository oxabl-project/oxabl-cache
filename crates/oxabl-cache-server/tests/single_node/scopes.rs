//! Who may do what: scopes, tenants, revoked and suspended credentials.

use crate::support::fixture::{World, create_cache, token};
use crate::support::frames::{self, Entry};
use crate::support::http::request;
use crate::support::server::Options;
use oxabl_cache_format as format;
use serde_json::json;

#[test]
fn a_read_token_cannot_write_and_a_developer_cannot_write_trusted() {
    let world = World::start("scopes", Options::default());
    let reserved = world.reserve(&world.reader, "trusted", 4_096);
    assert_eq!(
        (reserved.status, reserved.error_code().as_str()),
        (403, "forbidden")
    );
    let reserved = world.reserve(&world.dev, "trusted", 4_096);
    assert_eq!(reserved.status, 403, "write:personal is not write:trusted");
    let reserved = world.reserve(&world.ci, "personal", 4_096);
    assert_eq!(reserved.status, 403, "write:trusted is not write:personal");

    let pack = frames::pack(world.cache_bytes, &[3_000]);
    let slot = world
        .reserve(&world.dev, "personal", pack.bytes.len())
        .json()["packs"][0]
        .clone();
    assert_eq!(slot["status"], "ok");
    let put = world.put_slot(&slot, &pack.bytes);
    assert_eq!(put.status, 200);
    let pack_id = slot["pack_id"].as_str().expect("id");
    let as_trusted = frames::commit(
        format::Namespace::Trusted,
        &pack,
        &[Entry::new(frames::lookup_id(), 0)],
    );
    assert_eq!(
        world.commit(&world.dev, pack_id, &as_trusted).status,
        403,
        "a developer cannot commit trusted"
    );
    assert_eq!(
        world.commit(&world.reader, pack_id, &as_trusted).status,
        403,
        "a reader cannot commit"
    );
    let as_personal = frames::commit(
        format::Namespace::Personal,
        &pack,
        &[Entry::new(frames::lookup_id(), 0)],
    );
    let own = world.commit(&world.dev, pack_id, &as_personal);
    assert_eq!(own.status, 200);
    assert_eq!(
        frames::statuses(&own.body),
        vec![format::CommitStatus::Committed]
    );

    let purge = world.reader.json(
        "POST",
        &world.path("/purge"),
        Some(&json!({"below_epoch": 1})),
    );
    assert_eq!(purge.status, 403);
    let admin_routes = world.reader.json("GET", "/v1/tenant/tokens", None);
    assert_eq!(
        admin_routes.status, 403,
        "tenant administration needs tenant:admin"
    );
}

#[test]
fn another_tenants_cache_is_not_found_in_both_directions() {
    let world = World::start("tenants", Options::default());
    let other_tenant = world.server.create_tenant("other", json!({}));
    let other_admin = world.server.tenant_admin(&other_tenant);
    let other_cache = create_cache(&other_admin, "elsewhere", json!({}));
    let other_reader = token(&other_admin, "reader", &["read"], &[&other_cache]);
    let other_reader = other_admin.with_token(other_reader["token"].as_str().expect("token"));

    let theirs = world
        .reader
        .json("GET", &format!("/v1/caches/{other_cache}"), None);
    assert_eq!(
        (theirs.status, theirs.error_code().as_str()),
        (404, "not_found")
    );
    let ours = other_reader.json("GET", &world.path(""), None);
    assert_eq!(ours.status, 404);
    let listed = world.admin.json("GET", "/v1/tenant/caches", None).json();
    assert_eq!(listed["caches"].as_array().expect("caches").len(), 1);
    let granting = world.admin.json(
        "POST",
        "/v1/tenant/tokens",
        Some(&json!({"name": "x", "scopes": ["read"], "caches": [other_cache]})),
    );
    assert_eq!(
        granting.status, 404,
        "a token cannot be scoped to another tenant's cache"
    );
}

#[test]
fn missing_wrong_revoked_and_suspended_credentials_are_refused() {
    let world = World::start("credentials", Options::default());
    let public = &world.server.public;
    let anonymous = request(public, "GET", &world.path(""), &[], b"");
    assert_eq!(
        (anonymous.status, anonymous.error_code().as_str()),
        (401, "unauthenticated")
    );
    assert_eq!(anonymous.header("www-authenticate"), Some("Bearer"));
    let garbage = world
        .reader
        .with_token("oxc_not_a_token")
        .json("GET", &world.path(""), None);
    assert_eq!(garbage.status, 401);
    let operator_on_public =
        world
            .reader
            .with_token(&world.server.operator)
            .json("GET", &world.path(""), None);
    assert_eq!(
        operator_on_public.status, 401,
        "an operator token is not a tenant credential"
    );
    let operator_id = world
        .server
        .operator
        .split('_')
        .nth(1)
        .expect("operator id")
        .to_owned();
    let forged = format!("oxo_{operator_id}_{}", "0".repeat(52));
    let wrong_secret = crate::support::server::call(
        &world.server.admin,
        &forged,
        "GET",
        "/admin/v1/tenants",
        None,
    );
    assert_eq!(
        wrong_secret.status, 401,
        "the id alone is not an operator credential"
    );
    let on_admin = crate::support::server::call(
        &world.server.admin,
        &world.reader.token,
        "GET",
        "/admin/v1/tenants",
        None,
    );
    assert_eq!(
        on_admin.status, 401,
        "a tenant token is not an operator credential"
    );

    let created = token(&world.admin, "short-lived", &["read"], &[&world.cache]);
    let short = world
        .reader
        .with_token(created["token"].as_str().expect("token"));
    assert_eq!(short.json("GET", &world.path(""), None).status, 200);
    let revoked = world.admin.json(
        "DELETE",
        &format!("/v1/tenant/tokens/{}", created["id"].as_str().expect("id")),
        None,
    );
    assert_eq!(revoked.status, 204);
    assert_eq!(
        short.json("GET", &world.path(""), None).status,
        401,
        "a revoked token stops working at once"
    );

    let suspended = world.server.admin_call(
        "PATCH",
        &format!("/admin/v1/tenants/{}", world.tenant),
        Some(&json!({"status": "suspended"})),
    );
    assert_eq!(suspended.status, 200);
    let refused = world.reader.json("GET", &world.path(""), None);
    assert_eq!(
        (refused.status, refused.error_code().as_str()),
        (403, "tenant_suspended")
    );
    world.server.admin_call(
        "PATCH",
        &format!("/admin/v1/tenants/{}", world.tenant),
        Some(&json!({"status": "active"})),
    );
    assert_eq!(world.reader.json("GET", &world.path(""), None).status, 200);
}
