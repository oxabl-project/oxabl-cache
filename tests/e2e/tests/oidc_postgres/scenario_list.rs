//! Every module the `oidc` target runs must also run here.

#[test]
fn every_oidc_scenario_runs_on_postgres() {
    let here = include_str!("main.rs");
    let sqlite = include_str!("../oidc/main.rs");
    let missing: Vec<&str> = sqlite
        .lines()
        .filter_map(|line| line.strip_prefix("mod "))
        .filter_map(|name| name.strip_suffix(';'))
        .filter(|name| !here.contains(&format!("oidc/{name}.rs")))
        .collect();
    assert!(missing.is_empty(), "not run on postgres: {missing:?}");
}
