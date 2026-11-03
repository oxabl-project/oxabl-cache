//! Two server processes on one database: both serve, the first write wins across them.

use crate::nodes::{Node, database_url, sql};
use crate::support::fixture::World;
use crate::support::frames::{self, Entry};
use crate::support::server::{Client, Options};
use oxabl_cache_format as format;
use serde_json::json;
use std::time::Duration;

const TRUSTED: format::Namespace = format::Namespace::Trusted;
const ROUNDS: usize = 10;
const PER_ROUND: usize = 40;

fn records(world: &World, client: &Client, keys: &[[u8; 32]]) -> Vec<Option<Vec<u8>>> {
    let (bytes, shape) = world.fetch(client, keys);
    let fetched = format::LookupResponse::decode(&bytes, &shape).expect("OXLR");
    fetched
        .hits
        .iter()
        .map(|hit| hit.record.as_ref().map(|found| found.record.to_vec()))
        .collect()
}

/// A pack for `keys` that is reserved and uploaded, with the frame that commits it.
struct Prepared {
    pack_id: String,
    frame: Vec<u8>,
    entries: Vec<Entry>,
}

fn prepare(world: &World, client: &Client, keys: &[[u8; 32]], size: usize) -> Prepared {
    let pack = frames::pack(world.cache_bytes, &vec![size; keys.len()]);
    let entries: Vec<Entry> = keys
        .iter()
        .enumerate()
        .map(|(index, key)| Entry::new(*key, index as u32))
        .collect();
    let reserved = world.reserve_bytes(client, "trusted", &pack.bytes);
    let slot = reserved.json()["packs"][0].clone();
    assert_eq!(slot["status"], "ok", "{slot}");
    assert_eq!(world.put_slot(&slot, &pack.bytes).status, 200);
    Prepared {
        pack_id: slot["pack_id"].as_str().expect("pack id").to_owned(),
        frame: frames::commit(TRUSTED, &pack, &entries),
        entries,
    }
}

fn commit(world: &World, client: &Client, prepared: &Prepared) -> Vec<format::CommitStatus> {
    let answer = world.commit(client, &prepared.pack_id, &prepared.frame);
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.body)
    );
    frames::statuses(&answer.body)
}

#[test]
fn writers_on_two_nodes_race_for_the_same_keys_and_every_key_has_one_winner() {
    let world = World::start("two-nodes-race", Options::default());
    let node_b = Node::start(&world, "b");
    let (ci_a, ci_b) = (world.ci.clone(), node_b.client(&world.ci));
    let (reader_a, reader_b) = (world.reader.clone(), node_b.client(&world.reader));

    let rounds: Vec<Vec<[u8; 32]>> = (0..ROUNDS)
        .map(|_| (0..PER_ROUND).map(|_| frames::lookup_id()).collect())
        .collect();
    let prepared: Vec<(Prepared, Prepared)> = rounds
        .iter()
        .map(|keys| {
            (
                prepare(&world, &ci_a, keys, 2_000),
                prepare(&world, &ci_b, keys, 2_100),
            )
        })
        .collect();

    let mut statuses: Vec<(Vec<format::CommitStatus>, Vec<format::CommitStatus>)> = Vec::new();
    statuses.push({
        let a = commit(&world, &ci_a, &prepared[0].0);
        (a, commit(&world, &ci_b, &prepared[0].1))
    });
    statuses.push({
        let b = commit(&world, &ci_b, &prepared[1].1);
        (commit(&world, &ci_a, &prepared[1].0), b)
    });
    let barrier = std::sync::Barrier::new(2);
    let simultaneous = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            (2..ROUNDS)
                .map(|round| {
                    barrier.wait();
                    commit(&world, &ci_a, &prepared[round].0)
                })
                .collect::<Vec<_>>()
        });
        let b = scope.spawn(|| {
            (2..ROUNDS)
                .map(|round| {
                    barrier.wait();
                    commit(&world, &ci_b, &prepared[round].1)
                })
                .collect::<Vec<_>>()
        });
        a.join()
            .expect("node a")
            .into_iter()
            .zip(b.join().expect("node b"))
            .collect::<Vec<_>>()
    });
    statuses.extend(simultaneous);

    let mut live = 0u64;
    let mut won = (0, 0);
    for (round, (status_a, status_b)) in statuses.iter().enumerate() {
        let a_won = status_a
            .iter()
            .all(|s| *s == format::CommitStatus::Committed);
        let b_won = status_b
            .iter()
            .all(|s| *s == format::CommitStatus::Committed);
        assert!(
            a_won != b_won,
            "round {round}: one node won the whole pack: {status_a:?} {status_b:?}"
        );
        let loser = if a_won { status_b } else { status_a };
        assert!(
            loser.iter().all(|s| *s == format::CommitStatus::Exists),
            "round {round}: the loser is told every key exists"
        );
        if a_won {
            won.0 += 1;
            live += 2_000 * PER_ROUND as u64;
        } else {
            won.1 += 1;
            live += 2_100 * PER_ROUND as u64;
        }
    }
    assert!(won.0 > 0 && won.1 > 0, "both nodes won rounds: {won:?}");

    let keys: Vec<[u8; 32]> = rounds.iter().flatten().copied().collect();
    let from_a = records(&world, &reader_a, &keys);
    let from_b = records(&world, &reader_b, &keys);
    for (round, (status_a, _)) in statuses.iter().enumerate() {
        let a_won = status_a[0] == format::CommitStatus::Committed;
        let (winner, _) = if a_won {
            (&prepared[round].0, ())
        } else {
            (&prepared[round].1, ())
        };
        for (index, entry) in winner.entries.iter().enumerate() {
            let at_key = round * PER_ROUND + index;
            assert_eq!(
                from_a[at_key].as_deref(),
                Some(entry.record.as_slice()),
                "node a serves the winner of round {round} key {index}"
            );
            assert_eq!(
                from_b[at_key].as_deref(),
                Some(entry.record.as_slice()),
                "node b serves the winner of round {round} key {index}"
            );
        }
    }
    assert_eq!(world.info(&reader_a)["quota"]["live_bytes"], live);
    assert_eq!(world.info(&reader_b)["quota"]["live_bytes"], live);
    let rows = sql(
        &database_url(&world),
        "SELECT (SELECT count(*) FROM entries), (SELECT count(*) FROM segments WHERE dead_since IS NULL)",
    );
    let total = (ROUNDS * PER_ROUND) as i64;
    assert_eq!(rows[0].get::<_, i64>(0), total, "one entry per key");
    assert_eq!(rows[0].get::<_, i64>(1), total, "one live segment per key");
}

#[test]
fn the_request_rate_of_a_tenant_is_shared_by_the_live_nodes() {
    let world = World::build(
        "two-nodes-rate",
        Options::default(),
        json!({"rate": 1, "burst": 40}),
        json!({}),
    );
    let node_b = Node::start(&world, "b");
    let reader_b = node_b.client(&world.reader);

    let nodes = sql(&database_url(&world), "SELECT count(*) FROM nodes");
    assert_eq!(nodes[0].get::<_, i64>(0), 2, "both nodes beat");

    std::thread::sleep(Duration::from_secs(11));
    let admitted = |client: &Client| {
        let mut served = 0;
        for _ in 0..80 {
            if client.json("GET", &world.path(""), None).status == 429 {
                break;
            }
            served += 1;
        }
        served
    };
    let (on_a, on_b) = (admitted(&world.reader), admitted(&reader_b));
    assert!(
        (15..=25).contains(&on_a) && (15..=25).contains(&on_b),
        "each node admits about half of the burst of 40: {on_a} and {on_b}"
    );
}
