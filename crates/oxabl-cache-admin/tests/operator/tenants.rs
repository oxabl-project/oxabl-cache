//! Tenant lifecycle, quotas, tokens and the flag rules.

use crate::support::admin::{bare, run};
use crate::support::server::Server;
use crate::{Tenant, call};
use serde_json::{Value, json};

#[test]
fn a_tenant_is_created_listed_shown_limited_suspended_and_resumed() {
    let server = Server::start("op-tenants", false);
    let created = run(
        &server,
        &[
            "tenant",
            "create",
            "acme",
            "--hard-quota",
            "20GiB",
            "--rate",
            "40",
        ],
    );
    assert_eq!(created.code(), 0, "{}", created.stderr());
    let again = run(
        &server,
        &[
            "--json",
            "tenant",
            "create",
            "acme",
            "--hard-quota",
            "20GiB",
            "--rate",
            "40",
        ],
    );
    assert_eq!(again.code(), 0, "the same tenant again is not an error");
    let id = again.json()["id"].as_str().expect("id").to_owned();
    let different = run(&server, &["tenant", "create", "acme", "--rate", "7"]);
    assert_eq!(different.code(), 1, "same name, other limits: refused");
    assert!(
        different.stderr().contains("conflict"),
        "{}",
        different.stderr()
    );

    let listed = run(&server, &["--json", "tenant", "list"]);
    assert_eq!(
        listed.json()["tenants"].as_array().expect("tenants").len(),
        1
    );
    let table = run(&server, &["tenant", "list"]);
    assert!(
        table.stdout().contains("acme") && table.stdout().contains(&id),
        "{}",
        table.stdout()
    );

    let by_name = run(&server, &["--json", "tenant", "show", "acme"]).json();
    let by_id = run(&server, &["--json", "tenant", "show", &id]).json();
    assert_eq!(by_name, by_id);
    assert_eq!(by_name["hard_bytes"], 20u64 << 30);
    assert_eq!(by_name["rate"], 40);
    assert_eq!(by_name["status"], "active");

    let quota = run(
        &server,
        &[
            "tenant",
            "quota",
            "acme",
            "--hard-quota",
            "50GiB",
            "--egress",
            "1TiB",
        ],
    );
    assert_eq!(quota.code(), 0, "{}", quota.stderr());
    let shown = run(&server, &["--json", "tenant", "show", &id]).json();
    assert_eq!(shown["hard_bytes"], 50u64 << 30, "the quota change shows");
    assert_eq!(shown["egress_bytes"], 1u64 << 40);
    assert_eq!(shown["rate"], 40, "a flag left out keeps its value");
    let cleared = run(
        &server,
        &["--json", "tenant", "quota", "acme", "--egress", "none"],
    )
    .json();
    assert_eq!(cleared["egress_bytes"], Value::Null);
    assert_eq!(
        cleared["hard_bytes"],
        50u64 << 30,
        "a flag left out keeps its value"
    );

    let missing = run(&server, &["tenant", "show", "nobody"]);
    assert_eq!((missing.code(), missing.stdout()), (1, String::new()));
}

#[test]
fn suspending_refuses_the_tenants_data_requests_and_resuming_restores_them() {
    let server = Server::start("op-suspend", false);
    let tenant = Tenant::create(&server, "acme");
    assert_eq!(tenant.fetch_cache_info(&server).status(), 200);

    let suspended = run(&server, &["--json", "tenant", "suspend", "acme"]);
    assert_eq!(suspended.code(), 0, "{}", suspended.stderr());
    assert_eq!(suspended.json()["status"], "suspended");
    let refused = tenant.fetch_cache_info(&server);
    assert_eq!(refused.status(), 403);
    assert_eq!(
        refused.json::<Value>().expect("json")["error"]["code"],
        "tenant_suspended"
    );

    let resumed = run(&server, &["tenant", "resume", &tenant.id]);
    assert_eq!(resumed.code(), 0, "{}", resumed.stderr());
    assert_eq!(tenant.fetch_cache_info(&server).status(), 200);
}

#[test]
fn a_created_token_works_against_the_tenant_api_and_scopes_and_lifetimes_are_checked() {
    let server = Server::start("op-token", false);
    run(&server, &["tenant", "create", "acme"]);
    let minted = run(
        &server,
        &[
            "--json",
            "token",
            "create",
            "--tenant",
            "acme",
            "--expires",
            "30d",
        ],
    );
    assert_eq!(minted.code(), 0, "{}", minted.stderr());
    let token = minted.json()["token"].as_str().expect("token").to_owned();
    let listed = call(
        &server.public_url(),
        &token,
        "GET",
        "/v1/tenant/caches",
        None,
    );
    assert_eq!(
        listed.status(),
        200,
        "the minted token is a tenant:admin token"
    );
    let human = run(&server, &["token", "create", "--tenant", "acme"]);
    assert!(
        human
            .stdout()
            .lines()
            .any(|line| line.starts_with("token") && line.len() > 20),
        "{}",
        human.stdout()
    );

    let scopes = run(
        &server,
        &["token", "create", "--tenant", "acme", "--scopes", "read"],
    );
    assert_eq!(scopes.code(), 2);
    let lifetime = run(
        &server,
        &["token", "create", "--tenant", "acme", "--expires", "soon"],
    );
    assert_eq!(lifetime.code(), 2);
    let unknown = run(&server, &["token", "create", "--tenant", "nobody"]);
    assert_eq!(unknown.code(), 1);
}

#[test]
fn scrub_and_health_report_the_server() {
    let server = Server::start("op-scrub", false);
    let tenant = Tenant::create(&server, "acme");
    let scrub = run(&server, &["--json", "cache", "scrub", &tenant.cache]);
    assert_eq!(scrub.code(), 0, "{}", scrub.stderr());
    assert_eq!(
        scrub.json(),
        json!({"corrupt_segments": 0, "entries_removed": 0})
    );
    let health = run(&server, &["--json", "health"]);
    assert_eq!(health.json()["metadata"], "ok");
    assert_eq!(health.json()["gc_leader"], true);
    assert!(run(&server, &["health"]).stdout().contains("store"));
    assert_eq!(run(&server, &["cache", "scrub", "../x"]).code(), 2);
}

#[test]
fn missing_settings_and_bad_flags_are_usage_errors() {
    let none = bare(&["health"], &[]);
    assert_eq!(none.code(), 2);
    assert!(none.stderr().contains("--server"), "{}", none.stderr());
    let no_token = bare(&["--server", "http://127.0.0.1:9", "health"], &[]);
    assert_eq!(no_token.code(), 2);
    assert!(no_token.stderr().contains("token"), "{}", no_token.stderr());
    let scheme = bare(
        &[
            "--server",
            "127.0.0.1:9",
            "--token-file",
            "/nonexistent",
            "health",
        ],
        &[],
    );
    assert_eq!(scheme.code(), 2);
    let unreadable = bare(
        &["--token-file", "/nonexistent"],
        &[("OXABL_CACHE_ADMIN_SERVER", "http://127.0.0.1:9")],
    );
    assert_eq!(unreadable.code(), 2);
    assert_eq!(
        bare(&["tenant", "create", "x", "--hard-quota", "lots"], &[]).code(),
        2
    );
    assert_eq!(
        bare(&["--token", "secret", "health"], &[]).code(),
        2,
        "no token flag exists"
    );
    assert_eq!(bare(&["--help"], &[]).code(), 0);
}
