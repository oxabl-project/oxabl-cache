//! `[metadata] statement_timeout` and `idle_in_transaction_timeout` bound how long a request waits on the database.

use crate::nodes::sql;
use crate::support::backend::new_database;
use crate::support::fixture::{World, wait_until};
use crate::support::server::Options;
use oxabl_cache_e2e::fault_proxy::{Direction, Fault, FaultProxy};
use oxabl_cache_e2e::postgres::Postgres;
use std::time::{Duration, Instant};

fn info_status(world: &World) -> u16 {
    world.reader.json("GET", &world.path(""), None).status
}

#[test]
fn a_statement_stuck_on_a_lock_is_cancelled_after_the_configured_statement_timeout() {
    let database = new_database();
    let direct = format!("{}?sslmode=disable", database.url());
    let world = World::start(
        "pg-statement-timeout",
        Options {
            metadata_url: Some(direct.clone()),
            metadata_extra: "statement_timeout = \"1s\"",
            ..Options::default()
        },
    );
    assert_eq!(info_status(&world), 200);

    let holder = {
        let url = direct.clone();
        std::thread::spawn(move || {
            crate::support::objects::block_on(async {
                let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
                    .await
                    .expect("connect");
                tokio::spawn(connection);
                client
                    .batch_execute(
                        "BEGIN; LOCK TABLE caches IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(10); COMMIT",
                    )
                    .await
                    .expect("hold the lock");
            });
        })
    };
    assert!(
        wait_until(Duration::from_secs(20), || !sql(
            &direct,
            "SELECT 1 FROM pg_locks l JOIN pg_class c ON c.oid = l.relation
             WHERE c.relname = 'caches' AND l.mode = 'AccessExclusiveLock' AND l.granted"
        )
        .is_empty()),
        "the lock is held"
    );
    let started = Instant::now();
    let status = info_status(&world);
    assert_eq!(status, 503, "the request is refused, not stuck");
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "it was cancelled by the statement timeout, not freed by the lock: {:?}",
        started.elapsed()
    );
    holder.join().expect("lock holder");
    assert_eq!(
        info_status(&world),
        200,
        "the server recovers once the lock is gone"
    );
}

#[test]
fn a_session_left_idle_in_a_transaction_is_ended_after_the_configured_timeout() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let database = new_database();
    let proxy = runtime
        .block_on(FaultProxy::start(
            format!("127.0.0.1:{}", Postgres::shared().port())
                .parse()
                .expect("address"),
        ))
        .expect("proxy");
    let through_proxy = format!(
        "postgres://{}@127.0.0.1:{}/{}?sslmode=disable",
        database.user,
        proxy.addr().port(),
        database.name
    );
    let world = World::start(
        "pg-idle-timeout",
        Options {
            metadata_url: Some(through_proxy),
            metadata_extra: "idle_in_transaction_timeout = \"1s\"",
            ..Options::default()
        },
    );
    assert_eq!(info_status(&world), 200);

    proxy.set(Direction::Reply, Fault::Delay(Duration::from_secs(4)));
    assert_eq!(
        info_status(&world),
        503,
        "the database ended the session while the reply was still on its way"
    );
    proxy.set(Direction::Reply, Fault::Pass);
    assert!(
        wait_until(Duration::from_secs(30), || info_status(&world) == 200),
        "the pool replaces the ended session"
    );
}
