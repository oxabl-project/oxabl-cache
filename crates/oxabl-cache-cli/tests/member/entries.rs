//! `put`, `fetch` and `verify`: single entries through the command line, and a sample of
//! trusted entries fully verified in a scratch directory that is gone afterwards.

use std::fs;

use oxabl_cache_client::Mode;
use oxabl_cache_e2e::workdir::TestDir;

use crate::support::cache::store;
use crate::support::scenario::Owned;
use crate::support::sets::{self, key_hex};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn put_stores_one_key_and_fetch_installs_exactly_those_files() {
    let owned = Owned::start().await;
    let ci = owned.member("ci", "ci", &["read", "write:trusted"]).await;
    let dev = owned
        .member("dev", "developer", &["read", "write:personal"])
        .await;
    let work = TestDir::new("work").expect("work");
    let set = sets::make(&work.path().join("workspace"), "one", 1).remove(0);
    let key = key_hex(&set.key);
    let files: Vec<String> = set
        .refs
        .iter()
        .map(|r| format!("{}={}", r.path, r.disk_path.display()))
        .collect();
    let mut put = vec!["put", "--key", &key, "--mode", "ci"];
    for file in &files {
        put.extend(["--file", file]);
    }
    let stored = ci.cli(&put).ok().json();
    assert_eq!(stored["outcome"], "committed");
    assert!(stored["bytes_uploaded"].as_u64() > Some(0));
    assert_eq!(
        ci.cli(&put).ok().json()["outcome"],
        "existed",
        "the first write wins"
    );

    let stage = work.path().join("stage").display().to_string();
    let fetched = dev
        .cli(&["fetch", "--key", &key, "--stage", &stage])
        .ok()
        .json();
    assert_eq!(fetched["installed"], true);
    assert_eq!(fetched["tier"], "trusted");
    for (path, content) in &set.files {
        assert_eq!(
            fs::read(work.path().join("stage").join(path)).expect("installed"),
            *content
        );
    }
    let mut expect = vec!["fetch", "--key", &key, "--stage", &stage];
    for (path, _) in &set.files {
        expect.extend(["--expect", path]);
    }
    dev.cli(&expect).ok();

    let wrong = dev
        .cli(&[
            "fetch",
            "--key",
            &key,
            "--stage",
            &stage,
            "--expect",
            "r/other.r",
        ])
        .exits(1);
    assert!(wrong.stderr.contains("pathmismatch"), "{}", wrong.stderr);
    let unknown = key_hex(&sets::make(&work.path().join("workspace"), "none", 1)[0].key);
    let missing = dev
        .cli(&["fetch", "--key", &unknown, "--stage", &stage])
        .exits(1);
    assert!(missing.stderr.contains("notfound"), "{}", missing.stderr);
    dev.cli(&["fetch", "--key", "not-a-key", "--stage", &stage])
        .exits(2);
    ci.cli(&[
        "put",
        "--key",
        &key,
        "--file",
        "r/x.r=/nonexistent/file",
        "--mode",
        "ci",
    ])
    .exits(2);
    dev.cli(&["put", "--key", &key, "--file", "../escape=/nonexistent"])
        .exits(2);

    let own = sets::make(&work.path().join("own"), "own", 1).remove(0);
    let own_key = key_hex(&own.key);
    let own_files: Vec<String> = own
        .refs
        .iter()
        .map(|r| format!("{}={}", r.path, r.disk_path.display()))
        .collect();
    let mut personal = vec!["put", "--key", &own_key];
    for file in &own_files {
        personal.extend(["--file", file]);
    }
    assert_eq!(dev.cli(&personal).ok().json()["outcome"], "committed");
    let own_stage = work.path().join("own-stage").display().to_string();
    assert_eq!(
        dev.cli(&["fetch", "--key", &own_key, "--stage", &own_stage])
            .ok()
            .json()["tier"],
        "personal"
    );
    ci.cli(&[
        "fetch", "--key", &own_key, "--stage", &own_stage, "--mode", "ci",
    ])
    .exits(1);
    ci.cli(&[
        "put",
        "--key",
        &own_key,
        "--file",
        "r/x.r=/nonexistent",
        "--mode",
        "read-only",
    ])
    .exits(2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verify_fetches_a_sample_and_fails_unless_every_entry_is_a_verified_trusted_one() {
    let owned = Owned::start().await;
    let ci = owned.member("ci", "ci", &["read", "write:trusted"]).await;
    let dev = owned
        .member("dev", "developer", &["read", "write:personal"])
        .await;
    let work = TestDir::new("work").expect("work");
    let repo = owned.world.repo.path();
    let built = sets::make(&work.path().join("ci"), "built", 6);
    let cache = ci
        .as_member()
        .open(repo, Mode::Ci, &work.path().join("state"))
        .await
        .expect("open");
    store(&cache, &built).await;
    let mine = sets::make(&work.path().join("dev"), "mine", 1);
    let dev_cache = dev
        .as_member()
        .open(repo, Mode::Developer, &work.path().join("dev-state"))
        .await
        .expect("open");
    store(&dev_cache, &mine).await;

    let write_keys = |name: &str, extra: &[&sets::Set]| {
        let path = work.path().join(format!("keys-{name}"));
        let lines: Vec<String> = built
            .iter()
            .chain(extra.iter().copied())
            .map(|s| key_hex(&s.key))
            .collect();
        fs::write(&path, format!("# keys\n{}\n\n", lines.join("\n"))).expect("keys");
        path.display().to_string()
    };
    let good = write_keys("good", &[]);
    let all = ci
        .cli(&["verify", "--sample", "6", "--keys", &good])
        .ok()
        .json();
    assert_eq!(
        (all["sampled"].as_u64(), all["verified"].as_u64()),
        (Some(6), Some(6))
    );
    let few = ci
        .cli(&["verify", "--sample", "3", "--keys", &good])
        .ok()
        .json();
    assert_eq!(
        (few["sampled"].as_u64(), few["verified"].as_u64()),
        (Some(3), Some(3))
    );
    let more = ci
        .cli(&["verify", "--sample", "50", "--keys", &good])
        .ok()
        .json();
    assert_eq!(
        more["sampled"], 6,
        "a sample is never larger than the keys given"
    );

    let unknown = sets::make(&work.path().join("unknown"), "unknown", 1);
    let with_unknown = write_keys("unknown", &[&unknown[0]]);
    let failed = ci
        .cli(&["verify", "--sample", "7", "--keys", &with_unknown])
        .exits(1);
    assert!(failed.stderr.contains("1 of 7"), "{}", failed.stderr);
    assert!(failed.stdout.contains("notfound"), "{}", failed.stdout);

    let personal = write_keys("personal", &[&mine[0]]);
    let not_trusted = dev
        .cli(&["verify", "--sample", "7", "--keys", &personal])
        .exits(1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(not_trusted.stdout.trim()).expect("JSON")["not_trusted"],
        1,
        "a personal entry is not a verified trusted one"
    );

    ci.cli(&["verify", "--sample", "0", "--keys", &good])
        .exits(2);
    ci.cli(&[
        "verify",
        "--sample",
        "1",
        "--keys",
        &work.path().join("absent").display().to_string(),
    ])
    .exits(2);
    let scratch = ci.state.join("scratch");
    let leftover = fs::read_dir(&scratch).map(|d| d.count()).unwrap_or(0);
    assert_eq!(leftover, 0, "the scratch directories are removed");
}
