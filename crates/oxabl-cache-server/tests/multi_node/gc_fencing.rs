//! A garbage-collection leader that lost leadership while it was frozen cannot act on resume.

use crate::nodes::{Node, leads, sql};
use crate::support::backend::new_database;
use crate::support::fixture::{World, wait_until};
use crate::support::frames;
use crate::support::server::Options;
use oxabl_cache_e2e::fault_proxy::{Direction, Fault, FaultProxy};
use oxabl_cache_e2e::postgres::Postgres;
use std::process::Command;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(60);

fn signal(pid: u32, name: &str) {
    let status = Command::new("kill")
        .args([&format!("-{name}"), &pid.to_string()])
        .status()
        .expect("kill");
    assert!(status.success());
}

/// Ends the sessions of `url`'s database that hold the leadership lock and nothing
/// else. The cluster is shared with other tests, whose leaders must stay untouched.
fn terminate_leader_session(url: &str) {
    sql(
        url,
        "SELECT pg_terminate_backend(a.pid) FROM pg_stat_activity a
         WHERE a.datname = current_database()
           AND a.application_name = 'oxabl-cache-server' AND a.state = 'idle'
           AND a.pid IN (SELECT pid FROM pg_locks
                         WHERE locktype = 'advisory' AND granted AND database = a.datid)",
    );
}

fn pack_is_due(url: &str, pack: &str) -> bool {
    sql(
        url,
        &format!(
            "SELECT lease_until < floor(extract(epoch FROM now()))::bigint FROM packs
             WHERE encode(pack_id, 'hex') = '{pack}'"
        ),
    )
    .first()
    .is_some_and(|row| row.get::<_, bool>(0))
}

fn pack_state(url: &str, pack: &str) -> Option<i64> {
    sql(
        url,
        &format!("SELECT state FROM packs WHERE encode(pack_id, 'hex') = '{pack}'"),
    )
    .first()
    .map(|row| row.get(0))
}

#[test]
fn a_frozen_leader_that_lost_leadership_is_refused_on_resume_and_deletes_nothing() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let database = new_database();
    let proxy = runtime
        .block_on(FaultProxy::start(
            format!("127.0.0.1:{}", Postgres::shared().port())
                .parse()
                .expect("address"),
        ))
        .expect("proxy");
    let direct = format!("{}?sslmode=disable", database.url());
    let through_proxy = format!(
        "postgres://{}@127.0.0.1:{}/{}?sslmode=disable",
        database.user,
        proxy.addr().port(),
        database.name
    );
    let world = World::start(
        "gc-fencing",
        Options {
            metadata_url: Some(through_proxy),
            lease: "8s",
            ..Options::default()
        },
    );
    let old_leader = world.server.metrics.clone();
    assert!(
        wait_until(WAIT, || leads(&old_leader)),
        "the only node leads"
    );
    let rival = Node::start_with(
        &world,
        "rival",
        &[
            ("url = ", &format!("url = \"{direct}\"")),
            ("interval = ", "interval = \"1h\""),
        ],
    );
    assert!(
        !leads(&rival.metrics),
        "the rival waits while the first node leads"
    );

    let pack = frames::pack(world.cache_bytes, &[4_000]);
    let slot = world
        .reserve_bytes(&world.ci, "trusted", &pack.bytes)
        .json()["packs"][0]
        .clone();
    proxy.set(Direction::Reply, Fault::Delay(Duration::from_millis(1500)));
    assert_eq!(world.put_slot(&slot, &pack.bytes).status, 200);
    let pack_id = slot["pack_id"].as_str().expect("id").to_owned();

    assert!(
        wait_until(WAIT, || {
            let open = |direct: &str| {
                sql(
                    direct,
                    "SELECT count(*) FROM pg_stat_activity
                     WHERE application_name = 'oxabl-cache-server'
                       AND state LIKE 'idle in transaction%'",
                )[0]
                .get::<_, i64>(0)
            };
            open(&direct) > 0 && {
                std::thread::sleep(Duration::from_millis(1200));
                open(&direct) > 0
            }
        }),
        "the old leader sits inside a transaction of its slowed garbage-collection pass"
    );
    let leader_pid = world.server.pid();
    signal(leader_pid, "STOP");
    terminate_leader_session(&direct);
    wait_until(Duration::from_secs(4), || leads(&rival.metrics));
    assert!(
        wait_until(WAIT, || pack_is_due(&direct, &pack_id)),
        "the pack's lease runs out while the old leader is frozen"
    );
    signal(leader_pid, "CONT");
    assert!(
        wait_until(WAIT, || leads(&rival.metrics)),
        "the rival takes over once the lock is free and the leader's open transactions end"
    );
    proxy.set(Direction::Reply, Fault::Pass);

    assert!(
        wait_until(WAIT, || {
            !leads(&old_leader) || pack_state(&direct, &pack_id).is_none()
        }),
        "the old leader stops collecting before it deletes anything"
    );
    assert_eq!(
        pack_state(&direct, &pack_id),
        Some(0),
        "the expired pack the old leader would have swept is still there"
    );
    assert!(world.object_exists(&pack_id), "and so are its bytes");
    assert!(!leads(&old_leader), "the old leader no longer collects");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        pack_state(&direct, &pack_id),
        Some(0),
        "a pass resumed late deletes nothing either"
    );
    assert!(leads(&rival.metrics), "the rival leads");
}
