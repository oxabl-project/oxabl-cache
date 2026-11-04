//! Secrets and exit codes: passphrases and tokens come from a file or a terminal, nothing
//! secret is ever printed, and a mistake in the arguments or the files exits 2 while a refusal
//! exits 1.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use oxabl_cache_e2e::workdir::TestDir;

use crate::support::cli::{run, run_on_terminal};
use crate::support::person::{Person, write_secret};
use crate::support::scenario::Owned;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nothing_secret_is_printed_by_any_command() {
    let owned = Owned::start().await;
    let ci = owned.member("ci", "ci", &["read", "write:trusted"]).await;
    let work = TestDir::new("work").expect("work");
    let secret_file = work.path().join("ci.secret");
    let made = owned
        .owner
        .cli(&[
            "identity",
            "new",
            "--secret-out",
            &secret_file.display().to_string(),
        ])
        .ok();
    let identity_string = fs::read_to_string(&secret_file)
        .expect("secret")
        .trim()
        .to_owned();

    let mut printed = vec![made.everything()];
    let descriptor = made.json()["descriptor"]
        .as_str()
        .expect("descriptor")
        .to_owned();
    for args in [vec!["status"], vec!["rotate"]] {
        printed.push(owned.owner.cli(&args).ok().everything());
        printed.push(owned.owner.text(&args).ok().everything());
    }
    for (label, text) in [("second", false), ("third", true)] {
        let add = [
            "member",
            "add",
            &descriptor,
            "--label",
            label,
            "--role",
            "ci",
        ];
        let remove = ["member", "remove", label];
        for args in [&add[..], &remove[..]] {
            let run = if text {
                owned.owner.text(args)
            } else {
                owned.owner.cli(args)
            };
            printed.push(run.ok().everything());
        }
    }
    printed.push(ci.cli(&["status", "--mode", "ci"]).ok().everything());
    printed.push(
        owned
            .owner
            .cli(&[
                "fetch",
                "--key",
                &"ab".repeat(32),
                "--stage",
                &work.path().display().to_string(),
            ])
            .exits(1)
            .everything(),
    );
    printed.push(
        run(
            owned.owner.dir.path(),
            &[
                "status".to_owned(),
                "--passphrase-file".to_owned(),
                "/nonexistent".to_owned(),
            ],
            &[],
        )
        .exits(2)
        .everything(),
    );
    let everything = printed.join("\n");
    for (what, secret) in [
        ("the owner's passphrase", owned.owner.passphrase.as_str()),
        ("the owner's token", owned.owner.token.as_str()),
        ("the ci member's passphrase", ci.passphrase.as_str()),
        ("the ci member's token", ci.token.as_str()),
        ("an identity string", identity_string.as_str()),
    ] {
        assert!(!everything.contains(secret), "{what} was printed");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_secret_file_that_others_can_read_or_a_missing_passphrase_source_is_a_usage_error() {
    let owned = Owned::start().await;
    let loose = owned.owner.file("loose-passphrase");
    write_secret(&loose, &owned.owner.passphrase);
    fs::set_permissions(&loose, fs::Permissions::from_mode(0o644)).expect("mode");
    let mut person = Person::new("copy");
    person.use_repo(owned.world.repo.path());
    person.identity = owned.owner.identity.clone();
    person.passphrase_file = loose;
    person.use_token(&owned.owner.token);
    let refused = person.cli(&["status"]).exits(2);
    assert!(refused.stderr.contains("chmod 600"), "{}", refused.stderr);

    let loose_token = owned.owner.file("loose-token");
    write_secret(&loose_token, &owned.owner.token);
    fs::set_permissions(&loose_token, fs::Permissions::from_mode(0o640)).expect("mode");
    person.passphrase_file = owned.owner.passphrase_file.clone();
    person.token_file = Some(loose_token);
    person.cli(&["status"]).exits(2);

    let args: Vec<String> = [
        "status",
        "--repo",
        &owned.world.repo.path().display().to_string(),
        "--identity-file",
        &owned.owner.identity_arg(),
        "--token-file",
        &owned
            .owner
            .token_file
            .as_ref()
            .expect("token")
            .display()
            .to_string(),
    ]
    .iter()
    .map(|a| (*a).to_owned())
    .collect();
    let no_terminal = run(owned.owner.dir.path(), &args, &[]).exits(2);
    assert!(
        no_terminal.stderr.contains("--passphrase-file"),
        "{}",
        no_terminal.stderr
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_identity_is_encrypted_private_and_never_overwritten() {
    let person = Person::new("someone");
    let made = person
        .cli(&["identity", "new", "--file", &person.identity_arg()])
        .ok();
    let bytes = fs::read(&person.identity).expect("identity file");
    assert_eq!(
        fs::metadata(&person.identity)
            .expect("meta")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(bytes.starts_with(b"age-encryption.org/v1"), "an age file");
    assert!(
        !String::from_utf8_lossy(&bytes)
            .to_uppercase()
            .contains("OXABL-CACHE-IDENTITY"),
        "no plaintext identity inside"
    );
    let again = person
        .cli(&["identity", "new", "--file", &person.identity_arg()])
        .exits(1);
    assert!(
        again.stderr.contains("never overwritten"),
        "{}",
        again.stderr
    );
    assert_eq!(
        fs::read(&person.identity).expect("identity file"),
        bytes,
        "left intact"
    );
    assert!(
        made.json()["descriptor"]
            .as_str()
            .expect("descriptor")
            .starts_with("oxabl-cache-member-1")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_passphrase_is_refused_and_every_failure_has_its_exit_code() {
    let mut owned = Owned::start().await;
    let wrong = owned.owner.file("wrong");
    write_secret(&wrong, "not the passphrase");
    let right = std::mem::replace(&mut owned.owner.passphrase_file, wrong);
    let refused = owned.owner.cli(&["status"]).exits(1);
    assert!(
        refused.stderr.contains("cannot unlock the identity file"),
        "{}",
        refused.stderr
    );
    owned.owner.passphrase_file = right;

    owned.owner.cli(&["no-such-command"]).exits(2);
    owned
        .owner
        .cli(&["member", "add", "garbage", "--label", "x", "--role", "ci"])
        .exits(2);
    owned
        .owner
        .cli(&[
            "member", "add", "garbage", "--label", "x", "--role", "owner",
        ])
        .exits(2);
    let nowhere = TestDir::new("nowhere").expect("directory");
    let mut stranger = Person::new("stranger");
    stranger.identity = owned.owner.identity.clone();
    stranger.passphrase_file = owned.owner.passphrase_file.clone();
    let args: Vec<String> = [
        "status",
        "--identity-file",
        &stranger.identity_arg(),
        "--passphrase-file",
        &stranger.passphrase_file.display().to_string(),
        "--repo",
        &nowhere.path().display().to_string(),
    ]
    .iter()
    .map(|a| (*a).to_owned())
    .collect();
    run(stranger.dir.path(), &args, &[]).exits(2);

    let unreachable = owned.world.checkout(&format!(
        "server = \"http://127.0.0.1:1\"\ntenant = \"{}\"\ncache = \"{}\"\nowner_keys = [\"ed25519:{}\"]\nmin_roster_version = 0\n",
        owned.world.tenant,
        owned.cache,
        "A".repeat(43),
    ));
    let mut offline = Person::new("offline");
    offline.identity = owned.owner.identity.clone();
    offline.passphrase_file = owned.owner.passphrase_file.clone();
    offline.use_token(&owned.owner.token);
    offline.use_repo(unreachable.path());
    let down = offline.cli(&["status"]).exits(1);
    assert!(down.stdout.contains("\"on\":false"), "{}", down.stdout);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_passphrase_typed_on_the_terminal_is_not_echoed_and_must_be_typed_twice_for_a_new_identity()
 {
    let owned = Owned::start().await;
    let strings = |args: &[&str]| -> Vec<String> { args.iter().map(|a| (*a).to_owned()).collect() };
    let status = strings(&[
        "status",
        "--json",
        "--repo",
        &owned.world.repo.path().display().to_string(),
        "--identity-file",
        &owned.owner.identity_arg(),
        "--token-file",
        &owned
            .owner
            .token_file
            .as_ref()
            .expect("token")
            .display()
            .to_string(),
        "--state-dir",
        &owned.owner.state.display().to_string(),
    ]);
    let (code, shown) =
        run_on_terminal(owned.owner.dir.path(), &status, &[&owned.owner.passphrase]);
    assert_eq!(code, 0, "{shown}");
    assert!(shown.contains("Identity passphrase: "), "{shown}");
    assert!(
        !shown.contains(&owned.owner.passphrase),
        "the terminal echoed the passphrase:\n{shown}"
    );

    let (code, shown) = run_on_terminal(owned.owner.dir.path(), &status, &["not the passphrase"]);
    assert_eq!(code, 1, "{shown}");
    assert!(!shown.contains("not the passphrase"), "{shown}");

    let fresh = owned.owner.file("fresh-identity");
    let new = strings(&["identity", "new", "--file", &fresh.display().to_string()]);
    let (code, shown) = run_on_terminal(
        owned.owner.dir.path(),
        &new,
        &["a new passphrase", "a different one"],
    );
    assert_eq!(code, 2, "{shown}");
    assert!(
        !fresh.exists(),
        "no identity is made from two different entries"
    );
    let (code, shown) = run_on_terminal(
        owned.owner.dir.path(),
        &new,
        &["a new passphrase", "a new passphrase"],
    );
    assert_eq!(code, 0, "{shown}");
    assert!(!shown.contains("a new passphrase"), "{shown}");
    assert!(fresh.exists());
}
