//! A database that crashes in the middle of a commit: clients see errors, never a half-committed pack.

use crate::nodes::{fetch_status, sql};
use crate::support::fixture::{World, wait_until};
use crate::support::frames::{self, Entry};
use crate::support::server::Options;
use oxabl_cache_e2e::fault_proxy::{Direction, Fault, FaultProxy};
use oxabl_cache_e2e::postgres::Postgres;
use oxabl_cache_format as format;
use std::time::Duration;

const TRUSTED: format::Namespace = format::Namespace::Trusted;
const KEYS: usize = 400;

/// (state, segments, entries) of one pack, read straight from the database.
fn pack_rows(url: &str, pack: &str) -> (i64, i64, i64) {
    let rows = sql(
        url,
        &format!(
            "SELECT p.state,
                    (SELECT count(*) FROM segments s WHERE s.pack_id = p.pack_id),
                    (SELECT count(*) FROM entries e JOIN segments s
                       ON s.cache_id = e.cache_id AND s.segment_id = e.segment_id
                     WHERE s.pack_id = p.pack_id)
             FROM packs p WHERE encode(p.pack_id, 'hex') = '{pack}'"
        ),
    );
    (rows[0].get(0), rows[0].get(1), rows[0].get(2))
}

/// Whether the commit has written its segments: a server connection holds a
/// transaction id (its transaction wrote), or the segments are already visible.
fn segments_written(url: &str) -> bool {
    !sql(
        url,
        "SELECT 1 FROM pg_stat_activity
         WHERE application_name = 'oxabl-cache-server' AND backend_xid IS NOT NULL
         UNION ALL SELECT 1 FROM segments",
    )
    .is_empty()
}

#[test]
fn a_database_crash_during_a_commit_is_an_error_to_clients_and_leaves_no_half_pack() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut database = Postgres::launch().expect("private cluster");
    let created = runtime
        .block_on(database.create_database())
        .expect("create database");
    let proxy = runtime
        .block_on(FaultProxy::start(
            format!("127.0.0.1:{}", database.port())
                .parse()
                .expect("address"),
        ))
        .expect("proxy");
    let through_proxy = format!(
        "postgres://{}@127.0.0.1:{}/{}?sslmode=disable",
        created.user,
        proxy.addr().port(),
        created.name
    );
    let direct = format!("{}?sslmode=disable", created.url());
    let world = World::start(
        "database-crash",
        Options {
            metadata_url: Some(through_proxy),
            lease: "1h",
            ..Options::default()
        },
    );

    let keys: Vec<[u8; 32]> = (0..KEYS).map(|_| frames::lookup_id()).collect();
    let entries: Vec<Entry> = keys
        .iter()
        .enumerate()
        .map(|(index, key)| Entry::new(*key, index as u32))
        .collect();
    let pack = frames::pack(world.cache_bytes, &vec![1_000; KEYS]);
    let reserved = world.reserve_bytes(&world.ci, "trusted", &pack.bytes);
    let slot = reserved.json()["packs"][0].clone();
    assert_eq!(world.put_slot(&slot, &pack.bytes).status, 200);
    let pack_id = slot["pack_id"].as_str().expect("pack id").to_owned();
    let frame = frames::commit(TRUSTED, &pack, &entries);

    proxy.set(Direction::Reply, Fault::Delay(Duration::from_millis(300)));
    let first = std::thread::scope(|scope| {
        let commit = scope.spawn(|| world.commit(&world.ci, &pack_id, &frame));
        assert!(
            wait_until(Duration::from_secs(30), || segments_written(&direct)),
            "the commit reached its segment insert"
        );
        database.crash();
        let outcome = commit.join().expect("commit thread");
        let during = fetch_status(&world, &world.reader, &keys);
        assert_eq!(
            during, 503,
            "a lookup while the database is down is an error, so a miss"
        );
        database.restart().expect("restart");
        outcome
    });
    proxy.set(Direction::Reply, Fault::Pass);

    assert_eq!(
        first.status,
        503,
        "the interrupted commit is an error the client retries: {}",
        String::from_utf8_lossy(&first.body)
    );
    assert_eq!(first.error_code(), "unavailable");
    assert_eq!(
        pack_rows(&direct, &pack_id),
        (0, 0, 0),
        "an interrupted commit leaves the pack pending with no segment or entry"
    );

    assert!(
        wait_until(Duration::from_secs(30), || {
            fetch_status(&world, &world.reader, &keys) == 200
        }),
        "the server serves again once the database is back"
    );
    let (bytes, shape) = world.fetch(&world.reader, &keys);
    let missed = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
    assert!(
        missed.hits.iter().all(|hit| hit.record.is_none()),
        "nothing of the interrupted commit is visible"
    );

    let retried = world.commit(&world.ci, &pack_id, &frame);
    assert_eq!(
        retried.status,
        200,
        "{}",
        String::from_utf8_lossy(&retried.body)
    );
    assert_eq!(
        frames::statuses(&retried.body),
        vec![format::CommitStatus::Committed; KEYS]
    );
    assert_eq!(pack_rows(&direct, &pack_id), (1, KEYS as i64, KEYS as i64));
    let (bytes, shape) = world.fetch(&world.reader, &keys);
    let found = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
    for (hit, entry) in found.hits.iter().zip(&entries) {
        assert_eq!(
            hit.record.as_ref().map(|found| found.record.to_vec()),
            Some(entry.record.clone())
        );
    }
}
