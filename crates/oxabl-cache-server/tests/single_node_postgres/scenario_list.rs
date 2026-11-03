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
