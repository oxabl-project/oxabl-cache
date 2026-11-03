//! Every scenario file of `single_node` must also be included by this binary.

use std::path::Path;

#[test]
fn every_single_node_scenario_runs_on_postgres() {
    let included = include_str!("main.rs");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/single_node");
    let mut missing = Vec::new();
    for entry in std::fs::read_dir(dir).expect("scenario directory") {
        let path = entry.expect("entry").path();
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let is_scenario = path.extension().is_some_and(|ext| ext == "rs") && stem != "main";
        if is_scenario && !included.contains(&format!("single_node/{stem}.rs")) {
            missing.push(stem.to_owned());
        }
    }
    assert!(missing.is_empty(), "not run on postgres: {missing:?}");
}

#[test]
fn these_scenarios_run_on_a_postgres_database_with_the_current_schema() {
    use crate::support::fixture::World;
    use crate::support::server::Options;

    let world = World::start("backend-check", Options::default());
    let config = std::fs::read_to_string(world.server.dir.join("server.toml")).expect("config");
    let url = config
        .lines()
        .find_map(|line| line.strip_prefix("url = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("metadata url");
    assert!(url.starts_with("postgres://"), "{url}");
    let versions: Vec<i64> = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls)
                .await
                .expect("connect");
            tokio::spawn(connection);
            client
                .query(
                    "SELECT version FROM schema_migrations ORDER BY version",
                    &[],
                )
                .await
                .expect("versions")
                .iter()
                .map(|row| row.get(0))
                .collect()
        });
    assert_eq!(versions, vec![1, 2]);
}
