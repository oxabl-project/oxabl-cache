//! Tenant and operator administration over HTTP.

use crate::support::fixture::{World, create_cache, owner_key};
use crate::support::frames::{self, Entry};
use crate::support::http::request;
use crate::support::server::Options;
use oxabl_cache_format as format;
use serde_json::json;
use std::process::Command;

fn current_month() -> String {
    let output = Command::new("date")
        .args(["-u", "+%Y-%m"])
        .output()
        .expect("date");
    String::from_utf8(output.stdout)
        .expect("utf8")
        .trim()
        .to_owned()
}

#[test]
fn caches_users_grants_and_tokens_are_managed_with_natural_keys_and_no_secrets_in_listings() {
    let world = World::start("tenant-admin", Options::default());
    let admin = &world.admin;

    let again = admin.json(
        "POST",
        "/v1/tenant/caches",
        Some(&json!({"display_name": "codebase", "owner_keys": [owner_key()]})),
    );
    assert_eq!(again.status, 200, "the same cache again is a 200");
    assert_eq!(again.json()["id"], world.cache.as_str());
    let different = admin.json(
        "POST",
        "/v1/tenant/caches",
        Some(&json!({"display_name": "codebase", "owner_keys": [owner_key()], "hard_bytes": 5})),
    );
    assert_eq!(
        (different.status, different.error_code().as_str()),
        (409, "conflict")
    );
    let second = create_cache(admin, "second", json!({"retention_days": 7}));
    let listed = admin.json("GET", "/v1/tenant/caches", None).json();
    assert_eq!(listed["caches"].as_array().expect("caches").len(), 2);
    let kept = listed["caches"]
        .as_array()
        .expect("caches")
        .iter()
        .find(|c| c["id"] == second.as_str())
        .cloned();
    assert_eq!(kept.expect("second cache")["retention_days"], 7);
    let removed = admin.json("DELETE", &format!("/v1/tenant/caches/{second}"), None);
    assert_eq!(
        (removed.status, removed.error_code().as_str()),
        (501, "not_configured")
    );

    let keys = admin.json(
        "GET",
        &format!("/v1/tenant/caches/{second}/owner-keys"),
        None,
    );
    assert_eq!(keys.json()["owner_keys"], json!([owner_key()]));
    let replaced = json!({"owner_keys": ["01".repeat(32), "02".repeat(32)]});
    let path = format!("/v1/tenant/caches/{second}/owner-keys");
    assert_eq!(admin.json("PUT", &path, Some(&replaced)).status, 204);
    assert_eq!(
        admin.json("GET", &path, None).json()["owner_keys"]
            .as_array()
            .expect("keys")
            .len(),
        2
    );
    assert_eq!(
        admin
            .json("PUT", &path, Some(&json!({"owner_keys": []})))
            .status,
        400
    );
    assert_eq!(
        admin
            .json("PUT", &path, Some(&json!({"owner_keys": ["XYZ"]})))
            .status,
        400
    );

    let user = json!({"issuer": "https://idp.example", "subject": "alice", "label": "Alice"});
    let created = admin.json("POST", "/v1/tenant/users", Some(&user));
    assert_eq!(created.status, 201);
    let id = created.json()["id"].as_str().expect("id").to_owned();
    let repeat = admin.json("POST", "/v1/tenant/users", Some(&user));
    assert_eq!(
        (repeat.status, repeat.json()["id"].as_str()),
        (200, Some(id.as_str()))
    );
    let relabelled = admin.json(
        "POST",
        "/v1/tenant/users",
        Some(&json!({"issuer": "https://idp.example", "subject": "alice", "label": "Other"})),
    );
    assert_eq!(relabelled.status, 409);
    assert_eq!(
        admin.json("GET", "/v1/tenant/users", None).json()["users"]
            .as_array()
            .expect("users")
            .len(),
        1
    );

    let grant =
        json!({"principal": id, "cache": world.cache, "scopes": ["read", "write:personal"]});
    assert_eq!(
        admin.json("PUT", "/v1/tenant/grants", Some(&grant)).status,
        204
    );
    let grants = admin.json("GET", "/v1/tenant/grants", None).json();
    assert_eq!(
        grants["grants"][0]["scopes"],
        json!(["read", "write:personal"])
    );
    let tenant_wide = json!({"principal": id, "cache": null, "scopes": ["read"]});
    assert_eq!(
        admin
            .json("PUT", "/v1/tenant/grants", Some(&tenant_wide))
            .status,
        400,
        "a tenant-wide grant is tenant:admin alone"
    );
    let unknown = json!({"principal": id, "cache": world.cache, "scopes": ["root"]});
    assert_eq!(
        admin
            .json("PUT", "/v1/tenant/grants", Some(&unknown))
            .status,
        400
    );

    let issued = admin.json(
        "POST",
        "/v1/tenant/tokens",
        Some(&json!({"name": "alice-laptop", "scopes": ["read"], "caches": [world.cache], "principal": id, "expires_in_days": 30})),
    );
    assert_eq!(issued.status, 201);
    let issued = issued.json();
    assert_eq!(issued["principal"], id.as_str());
    let secret = issued["token"].as_str().expect("token").to_owned();
    let listing = admin.json("GET", "/v1/tenant/tokens", None);
    let text = String::from_utf8_lossy(&listing.body).into_owned();
    assert!(
        !text.contains(secret.rsplit('_').next().expect("secret")),
        "listings never carry secrets"
    );
    assert!(text.contains("alice-laptop"));
    let alice = admin.with_token(&secret);
    assert_eq!(alice.json("GET", &world.path(""), None).status, 200);
    assert_eq!(
        admin
            .json("DELETE", &format!("/v1/tenant/users/{id}"), None)
            .status,
        204
    );
    assert_eq!(
        alice.json("GET", &world.path(""), None).status,
        401,
        "deleting a user revokes their tokens"
    );

    let strict = admin.json(
        "POST",
        "/v1/tenant/users",
        Some(&json!({"issuer": "i", "subject": "s", "nickname": "x"})),
    );
    assert_eq!(strict.status, 400, "an unknown JSON field is refused");
    let no_scope = admin.json(
        "POST",
        "/v1/tenant/tokens",
        Some(&json!({"name": "x", "scopes": [], "caches": []})),
    );
    assert_eq!(no_scope.status, 400);
}

#[test]
fn oidc_answers_not_configured_until_an_identity_verifier_is_wired_in() {
    let world = World::start("oidc-stub", Options::default());
    let exchange = request(
        &world.server.public,
        "POST",
        "/v1/auth/oidc",
        &[("Content-Type", "application/json".to_owned())],
        json!({"tenant": world.tenant, "id_token": "a.b.c"})
            .to_string()
            .as_bytes(),
    );
    assert_eq!(
        (exchange.status, exchange.error_code().as_str()),
        (501, "not_configured")
    );
    let issuers = world.admin.json("GET", "/v1/tenant/oidc/issuers", None);
    assert_eq!(issuers.status, 501);
    let denied = world.reader.json("GET", "/v1/tenant/oidc/issuers", None);
    assert_eq!(denied.status, 403);
    let session = world.admin.json("DELETE", "/v1/auth/session", None);
    assert_eq!(session.status, 400, "an API token is not a session");
    let page = world.server.metrics_text();
    assert!(
        page.contains("oxabl_cache_oidc_exchanges_total{result=\"not_configured\"} 1"),
        "{page}"
    );
}

#[test]
fn operators_manage_tenants_quotas_health_and_export_usage() {
    let world = World::start("operator-admin", Options::default());
    let server = &world.server;
    let listed = server.admin_call("GET", "/admin/v1/tenants", None).json();
    assert_eq!(listed["tenants"][0]["name"], "acme");

    let same = server.admin_call("POST", "/admin/v1/tenants", Some(&json!({"name": "acme"})));
    assert_eq!(same.status, 200, "the same tenant again is a 200");
    let different = server.admin_call(
        "POST",
        "/admin/v1/tenants",
        Some(&json!({"name": "acme", "rate": 7})),
    );
    assert_eq!(different.status, 409);
    let quota = server.admin_call(
        "PUT",
        &format!("/admin/v1/tenants/{}/quota", world.tenant),
        Some(
            &json!({"hard_bytes": 1_000_000, "egress_bytes": 5_000_000, "rate": 80, "burst": 160}),
        ),
    );
    assert_eq!(quota.status, 200);
    assert_eq!(quota.json()["rate"], 80);
    let zero = server.admin_call(
        "PUT",
        &format!("/admin/v1/tenants/{}/quota", world.tenant),
        Some(&json!({"rate": 0, "burst": 1})),
    );
    assert_eq!(zero.status, 400);
    let bad = server.admin_call(
        "PATCH",
        &format!("/admin/v1/tenants/{}", world.tenant),
        Some(&json!({"status": "gone"})),
    );
    assert_eq!(bad.status, 400);
    let delete = server.admin_call(
        "DELETE",
        &format!("/admin/v1/tenants/{}", world.tenant),
        None,
    );
    assert_eq!(delete.status, 501);
    let unknown = server.admin_call(
        "PATCH",
        "/admin/v1/tenants/t_00000000000000000000000000",
        Some(&json!({"status": "active"})),
    );
    assert_eq!(unknown.status, 404);

    let health = server.admin_call("GET", "/admin/v1/health", None);
    assert_eq!(health.status, 200);
    assert_eq!(health.json()["gc_leader"], true);
    assert_eq!(health.json()["metadata"], "ok");

    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let key = frames::lookup_id();
    world.publish(
        &world.ci,
        format::Namespace::Trusted,
        &pack,
        &[Entry::new(key, 0)],
    );
    let (bytes, shape) = world.fetch(&world.reader, &[key, frames::lookup_id()]);
    let fetched = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
    assert_eq!(
        world
            .get_object(fetched.packs[0].url, Some("bytes=0-99"))
            .status,
        206
    );
    std::thread::sleep(std::time::Duration::from_millis(300));
    let month = current_month();
    let csv = server.admin_call(
        "GET",
        &format!("/admin/v1/usage?month={month}&format=csv"),
        None,
    );
    assert_eq!(csv.status, 200);
    let text = String::from_utf8(csv.body).expect("utf8");
    assert!(
        text.starts_with("month,tenant,cache,metric,value\n"),
        "{text}"
    );
    let row = |metric: &str| format!("{month},{},{},{metric},", world.tenant, world.cache);
    for (metric, value) in [
        ("ingress_bytes", pack.bytes.len() as u64),
        ("lookup_ids", 2),
        ("hits", 1),
        ("entries_committed", 1),
        ("egress_bytes", 4_100),
    ] {
        assert!(
            text.contains(&format!("{}{value}\n", row(metric))),
            "{metric} = {value} in:\n{text}"
        );
    }
    let own = world
        .admin
        .json("GET", &format!("/v1/tenant/usage?month={month}"), None)
        .json();
    let metrics: Vec<&str> = own["usage"]
        .as_array()
        .expect("usage")
        .iter()
        .filter_map(|row| row["metric"].as_str())
        .collect();
    assert!(
        metrics.contains(&"ingress_bytes") && metrics.contains(&"hits"),
        "{metrics:?}"
    );
    let bad_month = server.admin_call("GET", "/admin/v1/usage?month=nonsense", None);
    assert_eq!(bad_month.status, 400);

    let other = server.create_tenant("other", json!({}));
    let other_admin = server.tenant_admin(&other);
    let theirs = other_admin
        .json("GET", &format!("/v1/tenant/usage?month={month}"), None)
        .json();
    assert!(
        theirs["usage"].as_array().expect("usage").is_empty(),
        "a tenant sees only its own usage"
    );
}
