//! Liveness, readiness and the public health answer.

use crate::support::fixture::{World, wait_until};
use crate::support::http::request;
use crate::support::server::Options;
use serde_json::json;
use std::time::Duration;

#[test]
fn livez_and_readyz_answer_200_on_the_metrics_listener() {
    let world = World::start("health-probes", Options::default());
    for path in ["/livez", "/readyz"] {
        let mut answer = request(&world.server.metrics, "GET", path, &[], b"");
        assert!(
            wait_until(Duration::from_secs(30), || {
                answer = request(&world.server.metrics, "GET", path, &[], b"");
                answer.status == 200
            }),
            "{path} answers 200 once the server has started"
        );
        assert!(answer.body.is_empty(), "{path}");
    }
}

#[test]
fn the_probes_are_not_served_on_the_public_or_admin_listener() {
    let world = World::start("health-listeners", Options::default());
    for addr in [&world.server.public, &world.server.admin] {
        for path in ["/livez", "/readyz"] {
            assert_eq!(request(addr, "GET", path, &[], b"").status, 404, "{path}");
        }
    }
}

#[test]
fn the_public_health_route_is_204_without_a_body_or_a_credential() {
    let world = World::start("health-public", Options::default());
    let answer = request(&world.server.public, "GET", "/v1/health", &[], b"");
    assert_eq!(answer.status, 204);
    assert!(answer.body.is_empty());
    assert!(
        answer.header("x-request-id").is_some(),
        "it carries a request id like every route"
    );
    let with_garbage = request(
        &world.server.public,
        "GET",
        "/v1/health",
        &[("Authorization", "Bearer not-a-token".to_owned())],
        b"",
    );
    assert_eq!(
        with_garbage.status, 204,
        "a credential is neither needed nor read"
    );
}

#[test]
fn the_public_health_route_is_outside_the_tenant_rate_limit() {
    let world = World::build(
        "health-unlimited",
        Options::default(),
        json!({"rate": 1, "burst": 20}),
        json!({}),
    );
    let mut limited = false;
    for _ in 0..400 {
        if world.reader.json("GET", &world.path(""), None).status == 429 {
            limited = true;
            break;
        }
    }
    assert!(limited, "the tenant's burst runs out");
    for _ in 0..50 {
        let answer = request(&world.server.public, "GET", "/v1/health", &[], b"");
        assert_eq!(answer.status, 204);
    }
}
