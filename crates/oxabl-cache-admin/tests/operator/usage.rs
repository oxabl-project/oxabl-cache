//! The monthly usage export.

use crate::support::admin::run;
use crate::support::server::Server;
use crate::{Tenant, month_now};
use std::time::Duration;

#[test]
fn the_export_has_stable_columns_and_a_row_per_tenant_cache_and_metric() {
    let server = Server::start("op-usage", false);
    let acme = Tenant::create(&server, "acme");
    let beta = Tenant::create(&server, "beta");
    for _ in 0..3 {
        assert_eq!(acme.lookup(&server, 2).status(), 200);
    }
    assert_eq!(beta.lookup(&server, 1).status(), 200);
    std::thread::sleep(Duration::from_millis(300));
    let month = month_now();

    let csv = run(
        &server,
        &["usage", "export", "--month", &month, "--format", "csv"],
    );
    assert_eq!(csv.code(), 0, "{}", csv.stderr());
    let text = csv.stdout();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("month,tenant,cache,metric,value"));
    let rows: Vec<Vec<&str>> = lines.map(|line| line.split(',').collect()).collect();
    assert!(
        rows.iter().all(|row| row.len() == 5 && row[0] == month),
        "{text}"
    );
    let value = |tenant: &Tenant, metric: &str| -> u64 {
        rows.iter()
            .find(|row| row[1] == tenant.id && row[2] == tenant.cache && row[3] == metric)
            .unwrap_or_else(|| panic!("a {metric} row for {} in:\n{text}", tenant.id))[4]
            .parse()
            .expect("integer")
    };
    assert_eq!(value(&acme, "requests"), 3, "{text}");
    assert_eq!(value(&acme, "lookup_ids"), 6, "{text}");
    assert_eq!(value(&beta, "requests"), 1, "{text}");
    assert_eq!(value(&beta, "lookup_ids"), 1, "{text}");
    let mut keys: Vec<(&str, &str, &str)> = rows.iter().map(|r| (r[1], r[2], r[3])).collect();
    let sorted = {
        let mut copy = keys.clone();
        copy.sort();
        copy
    };
    assert_eq!(keys, sorted, "rows are sorted by tenant, cache, metric");
    keys.dedup();
    assert_eq!(
        keys.len(),
        rows.len(),
        "one row per tenant, cache and metric"
    );

    let default = run(&server, &["usage", "export", "--month", &month]);
    assert!(
        default
            .stdout()
            .starts_with("month,tenant,cache,metric,value\n")
    );
    let json = run(
        &server,
        &["usage", "export", "--month", &month, "--format", "json"],
    )
    .json();
    assert_eq!(json["month"], month);
    assert!(json["usage"].as_array().expect("usage").iter().any(|row| {
        row["tenant"] == acme.id && row["cache"] == acme.cache && row["metric"] == "requests"
    }));

    let empty = run(&server, &["usage", "export", "--month", "2001-01"]);
    assert_eq!(empty.stdout(), "month,tenant,cache,metric,value\n");
    for bad in ["2026-13", "2026-1", "last-month", "2026/10"] {
        assert_eq!(
            run(&server, &["usage", "export", "--month", bad]).code(),
            2,
            "{bad}"
        );
    }
}

#[test]
fn the_global_json_flag_makes_the_export_json_and_conflicts_with_csv() {
    let server = Server::start("op-usage-json", false);
    let acme = Tenant::create(&server, "acme");
    assert_eq!(acme.lookup(&server, 1).status(), 200);
    std::thread::sleep(Duration::from_millis(300));
    let month = month_now();

    let flag = run(&server, &["--json", "usage", "export", "--month", &month]);
    assert_eq!(flag.code(), 0, "{}", flag.stderr());
    let value = flag.json();
    assert_eq!(value["month"], month);
    assert!(value["usage"].as_array().expect("usage").iter().any(|row| {
        row["tenant"] == acme.id && row["cache"] == acme.cache && row["metric"] == "requests"
    }));

    let format = run(
        &server,
        &["usage", "export", "--month", &month, "--format", "json"],
    );
    assert_eq!(value, format.json(), "--json and --format json agree");

    let both = run(
        &server,
        &[
            "--json", "usage", "export", "--month", &month, "--format", "json",
        ],
    );
    assert_eq!(both.code(), 0, "{}", both.stderr());
    assert_eq!(value, both.json());

    let conflict = run(
        &server,
        &[
            "--json", "usage", "export", "--month", &month, "--format", "csv",
        ],
    );
    assert_eq!(conflict.code(), 2);
    assert!(conflict.stdout().is_empty(), "{}", conflict.stdout());
    assert!(
        conflict.stderr().contains("--format csv"),
        "{}",
        conflict.stderr()
    );
}
