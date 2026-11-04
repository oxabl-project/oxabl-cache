//! `/readyz` follows the database: 503 while it is down, 200 once it is back.

use crate::support::fixture::{World, wait_until};
use crate::support::http::request;
use crate::support::server::Options;
use oxabl_cache_e2e::postgres::Postgres;
use std::time::Duration;

fn readyz(world: &World) -> u16 {
    request(&world.server.metrics, "GET", "/readyz", &[], b"").status
}

#[test]
fn readyz_is_503_while_postgres_is_stopped_and_200_after_it_restarts() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut database = Postgres::launch().expect("private cluster");
    let created = runtime
        .block_on(database.create_database())
        .expect("create database");
    let world = World::start(
        "readiness",
        Options {
            metadata_url: Some(format!("{}?sslmode=disable", created.url())),
            ..Options::default()
        },
    );
    assert!(
        wait_until(Duration::from_secs(30), || readyz(&world) == 200),
        "the server is ready once it has started"
    );

    database.crash();
    assert!(
        wait_until(Duration::from_secs(30), || readyz(&world) == 503),
        "readiness fails once the database stops answering"
    );
    let down = request(&world.server.metrics, "GET", "/readyz", &[], b"");
    assert_eq!(down.error_code(), "unavailable");
    assert_eq!(
        request(&world.server.metrics, "GET", "/livez", &[], b"").status,
        200,
        "the process stays live while its database is down"
    );
    assert_eq!(
        request(&world.server.public, "GET", "/v1/health", &[], b"").status,
        204
    );

    database.restart().expect("restart");
    assert!(
        wait_until(Duration::from_secs(60), || readyz(&world) == 200),
        "readiness returns once the database is back"
    );
}
