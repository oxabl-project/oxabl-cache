//! `migrate --from sqlite://… --to postgres://…`.

use crate::support::fixture::{World, hex};
use crate::support::frames;
use crate::support::party::Party;
use crate::support::server::Options;
use crate::support::sqlite::on_sqlite;
use oxabl_cache_e2e::postgres::Postgres;
use oxabl_cache_format as format;
use sha2::{Digest, Sha256};
use std::fs;

const TABLES: [&str; 8] = [
    "tenants",
    "caches",
    "tokens",
    "packs",
    "segments",
    "entries",
    "entry_access",
    "operator_tokens",
];

fn target() -> String {
    let database = crate::support::objects::block_on(Postgres::shared().create_database())
        .expect("create database");
    format!("{}?sslmode=disable", database.url())
}

fn postgres_count(url: &str, table: &str) -> i64 {
    crate::support::objects::block_on(async {
        let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls)
            .await
            .expect("connect");
        tokio::spawn(connection);
        client
            .query_one(&format!("SELECT count(*) FROM {table}"), &[])
            .await
            .expect("count")
            .get(0)
    })
}

fn file_digest(path: &std::path::Path) -> String {
    hex(&Sha256::digest(fs::read(path).expect("file")))
}

/// Points the stopped server's configuration at `url` and starts it again.
fn switch_metadata(world: &mut World, url: &str) {
    let config = world.server.dir.join("server.toml");
    let text = fs::read_to_string(&config).expect("configuration");
    let switched: Vec<String> = text
        .lines()
        .map(|line| {
            if line.starts_with("url = ") {
                format!("url = \"{url}\"")
            } else {
                line.to_owned()
            }
        })
        .collect();
    fs::write(&config, switched.join("\n") + "\n").expect("configuration");
    world.server.restart();
    world.reconnect();
}

fn source_count(world: &World, table: &str) -> i64 {
    rusqlite::Connection::open_with_flags(
        world.server.dir.join("meta.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("open source")
    .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .expect("count")
}

fn migrate(world: &World, to: &str) -> std::process::Output {
    let from = format!("sqlite://{}/meta.db", world.server.dir.display());
    std::process::Command::new(env!("CARGO_BIN_EXE_oxabl-cache-server"))
        .args(["migrate", "--from", &from, "--to", to])
        .output()
        .expect("run migrate")
}

fn hits(party: &Party, keys: &[[u8; 32]]) -> usize {
    party.hits(keys).expect("lookup")
}

#[test]
fn a_populated_sqlite_database_is_copied_to_postgres_and_served_from_there_with_every_lookup_hitting()
 {
    let mut world = World::start(
        "migrate-populated",
        on_sqlite(
            "migrate-populated",
            Options {
                lease: "60s",
                ..Options::default()
            },
        ),
    );
    let second = Party::on(&world.tenant, &world.admin, "second");
    let stranger = Party::in_new_tenant(&world, "stranger");
    let (mut second_keys, mut stranger_keys) = (Vec::new(), Vec::new());
    for _ in 0..3 {
        second_keys.extend(second.publish(&world, 4).1);
        stranger_keys.extend(stranger.publish(&world, 3).1);
    }
    let pack = frames::pack(world.cache_bytes, &[3_000; 2]);
    let main_keys: Vec<[u8; 32]> = (0..2).map(|_| frames::lookup_id()).collect();
    let entries: Vec<frames::Entry> = main_keys
        .iter()
        .enumerate()
        .map(|(index, key)| frames::Entry::new(*key, index as u32))
        .collect();
    world.publish(&world.ci, format::Namespace::Trusted, &pack, &entries);
    let live_before = world.info(&world.reader)["quota"]["live_bytes"].clone();
    assert_eq!(
        world
            .server
            .admin_call("GET", "/admin/v1/usage?month=2000-01", None)
            .status,
        200,
        "usage is flushed into the ledger"
    );
    assert!(world.server.stop().success());
    let source = world.server.dir.join("meta.db");
    let source_digest = file_digest(&source);
    let source_counts: Vec<i64> = TABLES
        .iter()
        .map(|table| source_count(&world, table))
        .collect();
    assert!(
        source_counts.iter().all(|count| *count > 0),
        "{source_counts:?}"
    );

    let url = target();
    let output = migrate(&world, &url);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Vec<serde_json::Value> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line per table"))
        .collect();
    for (table, count) in TABLES.iter().zip(&source_counts) {
        let line = report
            .iter()
            .find(|line| line["table"] == *table)
            .expect("table line");
        assert_eq!(line["rows"], *count, "{table}");
        assert_eq!(postgres_count(&url, table), *count, "{table} in the target");
        assert_eq!(line["digest"].as_str().expect("digest").len(), 64);
    }
    assert_eq!(
        file_digest(&source),
        source_digest,
        "the source is not touched"
    );

    let again = migrate(&world, &url);
    assert!(
        !again.status.success(),
        "a target that holds rows is refused"
    );
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("not empty"),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    for (table, count) in TABLES.iter().zip(&source_counts) {
        assert_eq!(
            postgres_count(&url, table),
            *count,
            "{table} is left as it was"
        );
    }

    switch_metadata(&mut world, &url);
    let mut second = second;
    let mut stranger = stranger;
    second.reconnect(&world);
    stranger.reconnect(&world);
    assert_eq!(
        hits(&second, &second_keys),
        second_keys.len(),
        "every lookup hits"
    );
    assert_eq!(hits(&stranger, &stranger_keys), stranger_keys.len());
    let tenants = world.server.admin_call("GET", "/admin/v1/tenants", None);
    assert_eq!(tenants.status, 200, "the operator token came along");
    assert_eq!(
        tenants.json()["tenants"].as_array().expect("tenants").len(),
        2
    );
    assert_eq!(
        world.info(&world.reader)["quota"]["live_bytes"],
        live_before,
        "the counters came along"
    );
    let (frame, shape) = frames::lookup(&main_keys, true, true, true, false);
    let fetched = world.reader.binary("POST", &world.path("/lookup"), &frame);
    let found = format::LookupResponse::decode(&fetched.body, &shape).expect("OXLR");
    assert_eq!(
        found.hits.len(),
        main_keys.len(),
        "every lookup hits on Postgres"
    );
    let object = world.get_object(found.packs[0].url, None);
    assert_eq!(object.status, 200, "and its bytes are served");
    assert_eq!(object.body, pack.bytes);
    let (_, fresh) = second.publish(&world, 2);
    assert_eq!(
        hits(&second, &fresh),
        2,
        "the Postgres-backed server takes new writes"
    );
    let usage = world
        .server
        .admin_call("GET", "/admin/v1/usage?month=2000-01", None);
    assert_eq!(usage.status, 200);
}

#[test]
fn a_source_with_an_unknown_table_or_a_missing_file_is_refused_and_the_target_is_left_untouched() {
    let mut world = World::start(
        "migrate-refused",
        on_sqlite("migrate-refused", Options::default()),
    );
    assert!(world.server.stop().success());
    let url = target();
    let missing = std::process::Command::new(env!("CARGO_BIN_EXE_oxabl-cache-server"))
        .args([
            "migrate",
            "--from",
            "sqlite:///nonexistent/meta.db",
            "--to",
            &url,
        ])
        .output()
        .expect("run migrate");
    assert!(!missing.status.success());
    let wrong_url = std::process::Command::new(env!("CARGO_BIN_EXE_oxabl-cache-server"))
        .args(["migrate", "--from", "postgres://x/y", "--to", &url])
        .output()
        .expect("run migrate");
    assert!(!wrong_url.status.success());

    let source = world.server.dir.join("meta.db");
    let extra = rusqlite::Connection::open(&source).expect("open source");
    extra
        .execute_batch("CREATE TABLE surprise (id INTEGER PRIMARY KEY)")
        .expect("extra table");
    drop(extra);
    let refused = migrate(&world, &url);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("surprise"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    let created = crate::support::objects::block_on(async {
        let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .expect("connect");
        tokio::spawn(connection);
        client
            .query_one(
                "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public'",
                &[],
            )
            .await
            .expect("count")
            .get::<_, i64>(0)
    });
    assert_eq!(
        created, 0,
        "a refused migration leaves the target untouched"
    );
}
