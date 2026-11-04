//! The failure log under the state directory: every run appends what failed, `status` shows
//! the last few, and the log is private, bounded, free of secrets and never fails a run.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use oxabl_cache_client::{ArtifactFileRef, ArtifactPath, CompileKey, Mode};
use oxabl_cache_e2e::workdir::TestDir;
use serde_json::{Value, json};

use crate::support::cache::store;
use crate::support::cli::run;
use crate::support::person::Person;
use crate::support::scenario::Owned;
use crate::support::sets;

const UNREACHABLE_SERVER: &str = "http://127.0.0.1:1";

/// The owner on a checkout whose server does not answer.
struct Offline {
    person: Person,
    log: PathBuf,
    _checkout: TestDir,
}

fn offline(owned: &Owned) -> Offline {
    let checkout = owned.world.checkout(&format!(
        "server = \"{UNREACHABLE_SERVER}\"\ntenant = \"{}\"\ncache = \"{}\"\nowner_keys = [\"ed25519:{}\"]\nmin_roster_version = 0\n",
        owned.world.tenant,
        owned.cache,
        "A".repeat(43),
    ));
    let mut person = Person::new("offline");
    person.identity = owned.owner.identity.clone();
    person.passphrase_file = owned.owner.passphrase_file.clone();
    person.use_token(&owned.owner.token);
    person.use_repo(checkout.path());
    let log = person
        .state
        .join("errors")
        .join(format!("{}-{}.jsonl", owned.world.tenant, owned.cache));
    Offline {
        person,
        log,
        _checkout: checkout,
    }
}

fn entries(log: &PathBuf) -> Vec<Value> {
    fs::read_to_string(log)
        .expect("the log")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}")))
        .collect()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

fn seed(log: &PathBuf, count: usize, message_bytes: usize) {
    fs::create_dir_all(log.parent().expect("directory")).expect("directory");
    let lines: Vec<String> = (0..count)
        .map(|n| {
            json!({"t": 1_700_000_000 + n, "op": "old", "kind": "seeded",
                   "msg": format!("{n:03}{}", "x".repeat(message_bytes))})
            .to_string()
        })
        .collect();
    fs::write(log, format!("{}\n", lines.join("\n"))).expect("seed");
    fs::set_permissions(log, fs::Permissions::from_mode(0o644)).expect("mode");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_run_is_logged_privately_and_status_shows_the_last_failures() {
    let owned = Owned::start().await;
    let offline = offline(&owned);
    let first = offline.person.cli(&["status"]).exits(1);
    let reported = first.json();
    assert_eq!(reported["on"], false);
    let shown = reported["recent_errors"].as_array().expect("recent_errors");
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0]["operation"], "open");
    assert_eq!(shown[0]["kind"], "unreachable");
    assert!(
        shown[0]["message"]
            .as_str()
            .expect("message")
            .contains("unreachable"),
        "{shown:?}"
    );
    let at = shown[0]["at"].as_u64().expect("time");
    assert!(at.abs_diff(now()) < 120, "logged at {at}");

    offline.person.cli(&["status"]).exits(1);
    let text = offline.person.text(&["status"]).exits(1);
    assert!(text.stdout.contains("last errors"), "{}", text.stdout);
    assert!(text.stdout.contains("open unreachable"), "{}", text.stdout);
    let stamp = std::process::Command::new("date")
        .args(["-u", "-d", &format!("@{at}"), "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .expect("date");
    let stamp = String::from_utf8(stamp.stdout).expect("UTF-8");
    assert!(
        text.stdout.contains(stamp.trim()),
        "the first failure is shown at {stamp}:\n{}",
        text.stdout
    );
    assert!(
        text.stdout.matches("open unreachable").count() == 3,
        "three failures, one line each:\n{}",
        text.stdout
    );
    let five = {
        for _ in 0..4 {
            offline.person.cli(&["status"]).exits(1);
        }
        offline.person.cli(&["status"]).exits(1).json()
    };
    assert_eq!(five["recent_errors"].as_array().expect("errors").len(), 5);

    let log = entries(&offline.log);
    assert_eq!(log.len(), 8);
    for entry in &log {
        let keys: Vec<&String> = entry.as_object().expect("object").keys().collect();
        assert_eq!(keys.len(), 4, "{entry}");
    }
    let mode =
        |path: &std::path::Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(mode(&offline.log), 0o600, "the log is private");
    assert_eq!(
        mode(offline.log.parent().expect("directory")),
        0o700,
        "so is its directory"
    );

    let working = owned.owner.cli(&["status"]).ok().json();
    assert_eq!(working["on"], true);
    assert_eq!(working["recent_errors"], json!([]));
    let plain = owned.owner.text(&["status"]).ok();
    assert!(!plain.stdout.contains("last errors"), "{}", plain.stdout);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_library_logs_what_a_fetch_could_not_verify_and_status_reads_it() {
    let owned = Owned::start().await;
    let ci = owned.member("ci", "ci", &["read", "write:trusted"]).await;
    let dev = owned
        .member("dev", "developer", &["read", "write:personal"])
        .await;
    let work = TestDir::new("work").expect("work");
    let repo = owned.world.repo.path();
    let built = sets::make(&work.path().join("ci"), "built", 3);
    let ci_cache = ci
        .as_member()
        .open(repo, Mode::Ci, &work.path().join("ci-state"))
        .await
        .expect("open");
    store(&ci_cache, &built).await;
    owned.owner.cli(&["member", "remove", "ci"]).ok();

    let cache = dev
        .as_member()
        .open(repo, Mode::Developer, &dev.state)
        .await
        .expect("open");
    let stage = work.path().join("stage");
    let mut sampled = cache
        .fetch_sample(10, oxabl_cache_client::FetchOptions::new(stage.clone()))
        .await
        .expect("sample");
    let mut outcomes = Vec::new();
    while let Some(outcome) = sampled.next().await {
        outcomes.push(outcome);
    }
    assert_eq!(outcomes.len(), 3, "the server names every entry it holds");

    let in_library = cache.recent_errors(5);
    assert_eq!(in_library.len(), 1, "{in_library:?}");
    assert_eq!(in_library[0].operation, "fetch");
    assert!(
        in_library[0].message.starts_with("3 of 3 entries"),
        "{:?}",
        in_library[0]
    );
    let status = dev.cli(&["status"]).ok().json();
    let shown = status["recent_errors"].as_array().expect("recent_errors");
    assert_eq!(shown.len(), 1, "{status}");
    assert_eq!(shown[0]["operation"], "fetch");
    assert_eq!(shown[0]["kind"], in_library[0].kind.as_str());
    assert_eq!(shown[0]["message"], in_library[0].message.as_str());
    let text = dev.text(&["status"]).ok();
    assert!(text.stdout.contains("last errors"), "{}", text.stdout);

    let unknown = sets::make(&work.path().join("unknown"), "unknown", 2);
    let missing = crate::support::cache::fetch(&cache, &unknown, &stage).await;
    assert_eq!(missing.len(), 2);
    assert_eq!(
        cache.recent_errors(5).len(),
        1,
        "an entry that is simply not there is no failure"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_log_holds_no_secret_no_path_and_no_file_content() {
    let owned = Owned::start().await;
    let ci = owned.member("ci", "ci", &["read", "write:trusted"]).await;
    let work = TestDir::new("work").expect("work");
    let repo = owned.world.repo.path();
    let refused_token = "tok-secret-value-0123456789";
    let owner = &owned.owner;

    let args: Vec<String> = [
        "status",
        "--json",
        "--identity-file",
        &owner.identity_arg(),
        "--passphrase-file",
        &owner.passphrase_file.display().to_string(),
        "--repo",
        &repo.display().to_string(),
        "--state-dir",
        &owner.state.display().to_string(),
    ]
    .iter()
    .map(|a| (*a).to_owned())
    .collect();
    let refused = run(
        owner.dir.path(),
        &args,
        &[("OXABL_CACHE_TOKEN", refused_token)],
    )
    .exits(1);
    assert!(refused.stdout.contains("authentication"), "{refused}");

    let marker = "file-content-marker-0123456789";
    let outputs = work.path().join("outputs");
    let set = sets::make_text(&outputs, "vanish", 1, 2_000).remove(0);
    let disk = set.refs[0].disk_path.clone();
    let path = ArtifactPath::new("r/vanish/0.r").expect("path");
    fs::write(&disk, marker).expect("content");
    let doomed = ArtifactFileRef::from_disk(path, disk.clone()).expect("measured");
    let cache = ci
        .as_member()
        .open(repo, Mode::Ci, &ci.state)
        .await
        .expect("open");
    let uploader = cache.uploader().expect("uploader");
    fs::remove_file(&disk).expect("gone");
    uploader.put(CompileKey([9; 32]), vec![doomed]).await;
    let report = uploader.finish().await;
    assert_eq!(report.outcomes.len(), 1);

    let mut logs = String::new();
    for state in [&owner.state, &ci.state] {
        let dir = state.join("errors");
        for entry in fs::read_dir(&dir).expect("the log directory") {
            logs.push_str(&fs::read_to_string(entry.expect("entry").path()).expect("log"));
        }
    }
    assert!(logs.contains("authentication"), "{logs}");
    assert!(logs.contains("\"op\":\"store\""), "{logs}");
    let stored = ci.cli(&["status", "--mode", "ci"]).ok().json();
    assert_eq!(stored["recent_errors"][0]["operation"], "store");
    for (what, secret) in [
        ("the refused token", refused_token.to_owned()),
        ("the owner's token", owner.token.clone()),
        ("the owner's passphrase", owner.passphrase.clone()),
        (
            "a token file",
            owner
                .token_file
                .as_ref()
                .expect("token")
                .display()
                .to_string(),
        ),
        ("the identity file's path", owner.identity_arg()),
        ("the workspace path", work.path().display().to_string()),
        ("the file's path", disk.display().to_string()),
        ("file content", marker.to_owned()),
    ] {
        assert!(!logs.contains(&secret), "{what} is in the log:\n{logs}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_log_keeps_the_newest_entries_within_its_caps_and_fixes_its_mode() {
    let owned = Owned::start().await;
    let offline = offline(&owned);

    seed(&offline.log, 30, 20);
    offline.person.cli(&["status"]).exits(1);
    offline.person.cli(&["status"]).exits(1);
    let log = entries(&offline.log);
    assert_eq!(log.len(), 30, "at most 30 entries");
    assert_eq!(
        log[0]["msg"].as_str().expect("msg")[..3],
        *"002",
        "the oldest go first"
    );
    assert_eq!(log[29]["op"], "open");
    assert_eq!(log[28]["op"], "open");
    assert_eq!(log[27]["op"], "old");
    assert_eq!(
        fs::metadata(&offline.log)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "a log that was group readable is private again"
    );

    seed(&offline.log, 30, 200);
    offline.person.cli(&["status"]).exits(1);
    let bytes = fs::metadata(&offline.log).expect("metadata").len();
    assert!(bytes <= 4096, "{bytes} bytes");
    let log = entries(&offline.log);
    assert!(log.len() < 30 && log.len() > 10, "{}", log.len());
    assert_eq!(log[log.len() - 1]["op"], "open", "the newest stays");
    assert_eq!(log[log.len() - 2]["op"], "old");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_log_that_cannot_be_written_never_fails_the_run_it_records() {
    let owned = Owned::start().await;
    let offline = offline(&owned);
    let errors = offline.person.state.join("errors");
    fs::create_dir_all(&offline.person.state).expect("state");
    fs::write(&errors, "not a directory").expect("blocker");

    let blocked = offline.person.cli(&["status"]).exits(1);
    let json = blocked.json();
    assert_eq!(json["on"], false);
    assert!(
        json["reason"]
            .as_str()
            .expect("reason")
            .contains("unreachable"),
        "{json}"
    );
    assert_eq!(json["recent_errors"], json!([]));
    assert_eq!(fs::read(&errors).expect("untouched"), b"not a directory");

    let other = owned.owner.state.join("errors");
    fs::create_dir_all(&owned.owner.state).expect("state");
    fs::write(&other, "not a directory").expect("blocker");
    owned.owner.cli(&["status"]).ok();

    let corrupt = offline.log.clone();
    fs::remove_file(&errors).expect("unblock");
    fs::create_dir_all(corrupt.parent().expect("directory")).expect("directory");
    fs::write(&corrupt, "{not json\n\u{0}\u{1}garbage\n").expect("garbage");
    offline.person.cli(&["status"]).exits(1);
    let log = entries(&corrupt);
    assert_eq!(log.len(), 1, "garbage lines are dropped: {log:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writers_take_turns_and_one_that_waits_too_long_gives_up_without_failing() {
    let owned = Owned::start().await;
    let offline = offline(&owned);
    let dir = offline.log.parent().expect("directory");
    fs::create_dir_all(dir).expect("directory");
    let name = offline.log.file_name().expect("name").to_string_lossy();
    let lock = fs::File::create(dir.join(format!(".{name}.lock"))).expect("lock file");
    lock.lock().expect("lock");

    let held = offline.person.cli(&["status"]).exits(1);
    assert_eq!(held.json()["on"], false, "{held}");
    assert!(
        !offline.log.exists(),
        "a writer does not write while another holds the lock"
    );

    lock.unlock().expect("unlock");
    offline.person.cli(&["status"]).exits(1);
    assert_eq!(
        entries(&offline.log).len(),
        1,
        "once it is free the line lands"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn processes_that_fail_together_all_leave_their_line() {
    let owned = Owned::start().await;
    let offline = offline(&owned);
    let start = std::sync::Arc::new(std::sync::Barrier::new(20));
    let runs: Vec<_> = (0..20)
        .map(|_| {
            let start = start.clone();
            let home = offline.person.dir.path().to_path_buf();
            let mut args: Vec<String> = vec!["status".to_owned(), "--json".to_owned()];
            args.extend(offline.person.flags());
            std::thread::spawn(move || {
                start.wait();
                run(&home, &args, &[]).code
            })
        })
        .collect();
    for run in runs {
        assert_eq!(run.join().expect("run"), 1);
    }
    assert_eq!(entries(&offline.log).len(), 20, "no line is lost or torn");
}
