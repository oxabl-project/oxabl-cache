//! `migrate --from SOURCE.toml --to TARGET.toml`.

use crate::support::fixture::{World, hex};
use crate::support::frames;
use crate::support::objects::{Objects, block_on, s3_section};
use crate::support::party::Party;
use crate::support::server::Options;
use crate::support::sqlite::on_sqlite;
use oxabl_cache_e2e::garage::Garage;
use oxabl_cache_e2e::postgres::Postgres;
use oxabl_cache_format as format;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

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
    let database = block_on(Postgres::shared().create_database()).expect("create database");
    format!("{}?sslmode=disable", database.url())
}

fn postgres_count(url: &str, table: &str) -> i64 {
    block_on(async {
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

fn file_digest(path: &Path) -> String {
    hex(&Sha256::digest(fs::read(path).expect("file")))
}

/// The configuration of `world`'s server with its metadata url and, if given, its `[store]`
/// section replaced; written beside it as `name`.
fn config_for(world: &World, name: &str, url: &str, store: Option<&str>) -> PathBuf {
    let mut text = fs::read_to_string(world.server.dir.join("server.toml")).expect("configuration");
    text = text
        .lines()
        .map(|line| {
            if line.starts_with("url = ") {
                format!("url = \"{url}\"")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    if let Some(store) = store {
        let (start, end) = (
            text.find("[store]").expect("store section"),
            text.find("[keys]").expect("keys section"),
        );
        text.replace_range(start..end, store);
    }
    let path = world.server.dir.join(name);
    fs::write(&path, text).expect("configuration");
    path
}

fn fs_store(root: &Path) -> String {
    format!("[store]\nkind = \"fs\"\nroot = \"{}\"\n", root.display())
}

/// Runs the stopped server on `config` instead of its own.
fn switch_to(world: &mut World, config: &Path) {
    fs::copy(config, world.server.dir.join("server.toml")).expect("configuration");
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

fn live_packs(world: &World) -> i64 {
    rusqlite::Connection::open_with_flags(
        world.server.dir.join("meta.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("open source")
    .query_row("SELECT count(*) FROM packs WHERE state = 1", [], |row| {
        row.get(0)
    })
    .expect("count")
}

fn migrate_configs(from: &Path, to: &Path) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_oxabl-cache-server"))
        .arg("migrate")
        .arg("--from")
        .arg(from)
        .arg("--to")
        .arg(to)
        .output()
        .expect("run migrate")
}

fn migrate(world: &World, to: &Path) -> std::process::Output {
    migrate_configs(&world.server.dir.join("server.toml"), to)
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Every object of a local store: cache, pack and path.
fn objects_of(root: &Path) -> Vec<(String, String, PathBuf)> {
    let mut found = Vec::new();
    let Ok(caches) = fs::read_dir(root.join("v1")) else {
        return found;
    };
    for cache in caches {
        let cache = cache.expect("cache directory").path();
        for shard in fs::read_dir(&cache).expect("shards") {
            for file in fs::read_dir(shard.expect("shard").path()).expect("objects") {
                let path = file.expect("object").path();
                found.push((
                    cache
                        .file_name()
                        .expect("name")
                        .to_string_lossy()
                        .into_owned(),
                    path.file_name()
                        .expect("name")
                        .to_string_lossy()
                        .into_owned(),
                    path,
                ));
            }
        }
    }
    found
}

fn hits(party: &Party, keys: &[[u8; 32]]) -> usize {
    party.hits(keys).expect("lookup")
}

struct Populated {
    world: World,
    second: Party,
    stranger: Party,
    second_keys: Vec<[u8; 32]>,
    stranger_keys: Vec<[u8; 32]>,
    main_keys: Vec<[u8; 32]>,
    pack: frames::Pack,
    live_before: serde_json::Value,
}

/// A stopped server with two tenants, three caches' worth of entries, tokens and a flushed
/// usage ledger.
fn populated(name: &'static str) -> Populated {
    let mut world = World::start(
        name,
        on_sqlite(
            name,
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
    Populated {
        world,
        second,
        stranger,
        second_keys,
        stranger_keys,
        main_keys,
        pack,
        live_before,
    }
}

/// Everything the migrated deployment must still serve.
fn assert_served(state: &mut Populated) {
    let world = &state.world;
    state.second.reconnect(world);
    state.stranger.reconnect(world);
    assert_eq!(
        hits(&state.second, &state.second_keys),
        state.second_keys.len(),
        "every lookup hits"
    );
    assert_eq!(
        hits(&state.stranger, &state.stranger_keys),
        state.stranger_keys.len()
    );
    let tenants = world.server.admin_call("GET", "/admin/v1/tenants", None);
    assert_eq!(tenants.status, 200, "the operator token came along");
    assert_eq!(
        tenants.json()["tenants"].as_array().expect("tenants").len(),
        2
    );
    assert_eq!(
        world.info(&world.reader)["quota"]["live_bytes"],
        state.live_before,
        "the counters came along"
    );
    let (frame, shape) = frames::lookup(&state.main_keys, true, true, true, false);
    let fetched = world.reader.binary("POST", &world.path("/lookup"), &frame);
    let found = format::LookupResponse::decode(&fetched.body, &shape).expect("OXLR");
    assert_eq!(
        found.hits.len(),
        state.main_keys.len(),
        "every lookup hits on Postgres"
    );
    let object = world.get_object(found.packs[0].url, None);
    assert_eq!(object.status, 200, "and its bytes are served");
    assert_eq!(object.body, state.pack.bytes);
    let (_, fresh) = state.second.publish(world, 2);
    assert_eq!(
        hits(&state.second, &fresh),
        2,
        "the Postgres-backed server takes new writes"
    );
}

fn report(output: &std::process::Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line per table"))
        .collect()
}

fn line_for<'a>(report: &'a [serde_json::Value], table: &str) -> &'a serde_json::Value {
    report
        .iter()
        .find(|line| line["table"] == table)
        .expect("table line")
}

#[test]
fn a_populated_sqlite_database_is_copied_to_postgres_with_its_packs_and_served_from_there() {
    let mut state = populated("migrate-populated");
    let world = &state.world;
    let source = world.server.dir.join("meta.db");
    let source_digest = file_digest(&source);
    let source_counts: Vec<i64> = TABLES
        .iter()
        .map(|table| source_count(world, table))
        .collect();
    assert!(
        source_counts.iter().all(|count| *count > 0),
        "{source_counts:?}"
    );
    let source_objects = objects_of(&world.server.dir.join("objects"));
    assert_eq!(source_objects.len() as i64, live_packs(world));

    let url = target();
    let target_root = world.server.dir.join("objects-target");
    let config = config_for(world, "target.toml", &url, Some(&fs_store(&target_root)));
    let output = migrate(world, &config);
    assert!(output.status.success(), "{}", stderr(&output));
    let report = report(&output);
    for (table, count) in TABLES.iter().zip(&source_counts) {
        let line = line_for(&report, table);
        assert_eq!(line["rows"], *count, "{table}");
        assert_eq!(postgres_count(&url, table), *count, "{table} in the target");
        assert_eq!(line["digest"].as_str().expect("digest").len(), 64);
    }
    let packs = report
        .iter()
        .find(|line| line["packs"].is_number())
        .expect("pack line");
    assert_eq!(packs["packs"], source_objects.len());
    assert_eq!(packs["copied"], true);
    assert_eq!(
        file_digest(&source),
        source_digest,
        "the source is not touched"
    );
    let copied = objects_of(&target_root);
    assert_eq!(copied.len(), source_objects.len());
    for (cache, pack, path) in &source_objects {
        let twin = copied
            .iter()
            .find(|(c, p, _)| c == cache && p == pack)
            .expect("every live pack is in the target store");
        assert_eq!(
            fs::read(&twin.2).expect("copy"),
            fs::read(path).expect("source")
        );
    }

    let stamps: Vec<_> = copied
        .iter()
        .map(|(_, _, path)| {
            fs::metadata(path)
                .expect("object")
                .modified()
                .expect("mtime")
        })
        .collect();
    let again = migrate(world, &config);
    assert!(
        !again.status.success(),
        "a target that holds rows is refused"
    );
    assert!(stderr(&again).contains("not empty"), "{}", stderr(&again));
    for (table, count) in TABLES.iter().zip(&source_counts) {
        assert_eq!(
            postgres_count(&url, table),
            *count,
            "{table} is left as it was"
        );
    }
    let after: Vec<_> = objects_of(&target_root)
        .iter()
        .map(|(_, _, path)| {
            fs::metadata(path)
                .expect("object")
                .modified()
                .expect("mtime")
        })
        .collect();
    assert_eq!(after.len(), stamps.len());
    assert!(
        stamps.iter().all(|stamp| after.contains(stamp)),
        "a refused run writes no pack"
    );

    let world = &mut state.world;
    switch_to(world, &config);
    assert_served(&mut state);
}

#[test]
fn packs_are_copied_from_disk_into_a_bucket_and_served_from_there() {
    let mut state = populated("migrate-to-bucket");
    let world = &state.world;
    let source_objects = objects_of(&world.server.dir.join("objects"));
    let bucket = block_on(Garage::shared().new_bucket()).expect("bucket");
    let section = s3_section(&world.server.dir, &bucket);
    let url = target();
    let config = config_for(world, "target.toml", &url, Some(&section));
    let output = migrate(world, &config);
    assert!(output.status.success(), "{}", stderr(&output));

    let remote = Objects::S3 { bucket };
    for (cache, pack, path) in &source_objects {
        assert_eq!(
            remote.read(cache, pack),
            fs::read(path).expect("source"),
            "{pack} is in the bucket with its bytes"
        );
    }
    let world = &mut state.world;
    switch_to(world, &config);
    assert_served(&mut state);
}

#[test]
fn packs_that_stay_in_the_same_store_are_verified_and_left_alone() {
    let mut state = populated("migrate-in-place");
    let world = &state.world;
    let root = world.server.dir.join("objects");
    let before: Vec<_> = objects_of(&root)
        .into_iter()
        .map(|(_, _, path)| {
            let stamp = fs::metadata(&path)
                .expect("object")
                .modified()
                .expect("mtime");
            (path, stamp)
        })
        .collect();
    let url = target();
    let config = config_for(world, "target.toml", &url, None);
    let output = migrate(world, &config);
    assert!(output.status.success(), "{}", stderr(&output));
    let packs = report(&output)
        .into_iter()
        .find(|line| line["packs"].is_number())
        .expect("pack line");
    assert_eq!(packs["copied"], false);
    assert_eq!(packs["packs"], before.len());
    for (path, stamp) in &before {
        assert_eq!(
            &fs::metadata(path)
                .expect("object")
                .modified()
                .expect("mtime"),
            stamp,
            "{} is not rewritten",
            path.display()
        );
    }
    let world = &mut state.world;
    switch_to(world, &config);
    assert_served(&mut state);
}

#[test]
fn a_damaged_pack_in_the_source_refuses_the_migration_and_leaves_the_target_without_rows() {
    let state = populated("migrate-damaged");
    let world = &state.world;
    let root = world.server.dir.join("objects");
    let (_, name, path) = objects_of(&root).remove(0);
    let original = fs::read(&path).expect("object");
    let truncated = original[..original.len() - 7].to_vec();
    let mut flipped = original.clone();
    flipped[100] ^= 1;
    let mut footer_flipped = original.clone();
    let footer = footer_flipped.len() - 60;
    footer_flipped[footer] ^= 1;

    let mut run = 0;
    for (what, damage) in [
        ("a flipped byte in a segment", Some(flipped)),
        ("a flipped byte in the footer", Some(footer_flipped)),
        ("a truncated object", Some(truncated)),
        ("a missing object", None),
    ] {
        for in_place in [false, true] {
            match &damage {
                Some(bytes) => fs::write(&path, bytes).expect("damage"),
                None => {
                    let _ = fs::remove_file(&path);
                }
            }
            run += 1;
            let url = target();
            let section = fs_store(&world.server.dir.join(format!("objects-{run}")));
            let config = config_for(
                world,
                "target.toml",
                &url,
                (!in_place).then_some(&section[..]),
            );
            let output = migrate(world, &config);
            assert!(
                !output.status.success(),
                "{what} is refused (in place: {in_place})"
            );
            assert!(
                stderr(&output).contains(&name),
                "{what}: the message names the pack: {}",
                stderr(&output)
            );
            for table in TABLES {
                assert_eq!(
                    postgres_count(&url, table),
                    0,
                    "{what}: {table} has no row in the target (in place: {in_place})"
                );
            }
            fs::write(&path, &original).expect("restore");
        }
    }
    let url = target();
    let config = config_for(
        world,
        "target.toml",
        &url,
        Some(&fs_store(&world.server.dir.join("objects-ok"))),
    );
    let output = migrate(world, &config);
    assert!(
        output.status.success(),
        "the restored source migrates: {}",
        stderr(&output)
    );
}

#[test]
fn a_source_with_an_unknown_table_a_missing_file_or_a_wrong_target_is_refused_and_the_target_is_left_untouched()
 {
    let mut world = World::start(
        "migrate-refused",
        on_sqlite("migrate-refused", Options::default()),
    );
    assert!(world.server.stop().success());
    let url = target();
    let config = config_for(&world, "target.toml", &url, None);
    let missing = migrate_configs(&world.server.dir.join("absent.toml"), &config);
    assert!(!missing.status.success());
    let to_sqlite = migrate(&world, &world.server.dir.join("server.toml"));
    assert!(!to_sqlite.status.success(), "a SQLite target is refused");
    assert!(
        stderr(&to_sqlite).contains("Postgres"),
        "{}",
        stderr(&to_sqlite)
    );
    let from_postgres = migrate_configs(&config, &config);
    assert!(
        !from_postgres.status.success(),
        "a Postgres source is refused"
    );
    assert!(
        stderr(&from_postgres).contains("SQLite"),
        "{}",
        stderr(&from_postgres)
    );

    let source = world.server.dir.join("meta.db");
    let extra = rusqlite::Connection::open(&source).expect("open source");
    extra
        .execute_batch("CREATE TABLE surprise (id INTEGER PRIMARY KEY)")
        .expect("extra table");
    drop(extra);
    let refused = migrate(&world, &config);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("surprise"),
        "{}",
        stderr(&refused)
    );
    let created = block_on(async {
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
