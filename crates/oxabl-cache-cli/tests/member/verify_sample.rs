//! `verify --sample n` without a key file: the server names live trusted entries and the
//! client fetches and verifies each one.

use oxabl_cache_client::Mode;
use oxabl_cache_e2e::workdir::TestDir;

use crate::support::cache::store;
use crate::support::scenario::Owned;
use crate::support::sets;

fn counts(output: &serde_json::Value) -> (Option<u64>, Option<u64>) {
    (output["sampled"].as_u64(), output["verified"].as_u64())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_server_names_the_sample_and_every_entry_is_verified() {
    let owned = Owned::start().await;
    let ci = owned.member("ci", "ci", &["read", "write:trusted"]).await;
    let dev = owned
        .member("dev", "developer", &["read", "write:personal"])
        .await;
    let work = TestDir::new("work").expect("work");
    let repo = owned.world.repo.path();
    let built = sets::make(&work.path().join("ci"), "built", 6);
    let ci_cache = ci
        .as_member()
        .open(repo, Mode::Ci, &work.path().join("state"))
        .await
        .expect("open");
    store(&ci_cache, &built).await;
    let mine = sets::make(&work.path().join("dev"), "mine", 2);
    let dev_cache = dev
        .as_member()
        .open(repo, Mode::Developer, &work.path().join("dev-state"))
        .await
        .expect("open");
    store(&dev_cache, &mine).await;

    let three = ci.cli(&["verify", "--sample", "3"]).ok().json();
    assert_eq!(counts(&three), (Some(3), Some(3)));
    let all = ci.cli(&["verify", "--sample", "6"]).ok().json();
    assert_eq!(counts(&all), (Some(6), Some(6)));
    let more = ci.cli(&["verify", "--sample", "1000"]).ok().json();
    assert_eq!(
        counts(&more),
        (Some(6), Some(6)),
        "a sample is never larger than the trusted entries"
    );
    let developer = dev.cli(&["verify", "--sample", "1000"]).ok().json();
    assert_eq!(
        (counts(&developer), developer["not_trusted"].as_u64()),
        ((Some(6), Some(6)), Some(0)),
        "personal entries are never sampled"
    );
    let text = dev.text(&["verify", "--sample", "2"]).ok();
    assert!(text.stdout.contains("verified"), "{}", text.stdout);

    owned.owner.cli(&["member", "remove", "ci"]).ok();
    let removed = dev.cli(&["verify", "--sample", "6"]).exits(1);
    let json = removed.json();
    assert_eq!(counts(&json), (Some(6), Some(0)));
    let missed: u64 = json["misses"]
        .as_object()
        .expect("misses")
        .values()
        .filter_map(serde_json::Value::as_u64)
        .sum();
    assert_eq!(missed, 6, "every sampled entry is named as a miss");
    assert!(removed.stderr.contains("6 of 6"), "{}", removed.stderr);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_empty_cache_verifies_nothing_and_that_is_a_failure() {
    let owned = Owned::start().await;
    let ci = owned.member("ci", "ci", &["read", "write:trusted"]).await;
    let empty = ci.cli(&["verify", "--sample", "5"]).exits(1);
    assert_eq!(counts(&empty.json()), (Some(0), Some(0)));
    assert!(
        empty.stderr.contains("nothing was verified"),
        "{}",
        empty.stderr
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sample_size_outside_one_to_a_thousand_is_a_usage_error() {
    let owned = Owned::start().await;
    let ci = owned.member("ci", "ci", &["read", "write:trusted"]).await;
    for size in ["0", "1001", "100000"] {
        let refused = ci.cli(&["verify", "--sample", size]).exits(2);
        assert!(refused.stderr.contains("--sample"), "{}", refused.stderr);
    }
    ci.cli(&["verify", "--sample", "1000"]).exits(1);
    ci.cli(&["verify", "--sample", "-1"]).exits(2);
}
