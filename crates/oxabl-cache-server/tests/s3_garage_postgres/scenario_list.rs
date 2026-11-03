//! Every module `s3_garage` runs must also run here.

#[test]
fn every_garage_scenario_runs_on_postgres() {
    let here = include_str!("main.rs");
    let garage = include_str!("../s3_garage/main.rs");
    let missing: Vec<&str> = garage
        .lines()
        .filter(|line| line.starts_with("#[path = "))
        .filter(|line| !here.contains(*line))
        .collect();
    assert!(missing.is_empty(), "not run on postgres: {missing:?}");
}
