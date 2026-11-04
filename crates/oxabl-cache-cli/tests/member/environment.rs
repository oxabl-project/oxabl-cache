//! Secrets from the environment: `OXABL_CACHE_TOKEN`, `OXABL_CACHE_IDENTITY` and
//! `OXABL_CACHE_IDENTITY_PASSPHRASE` stand in for files, a file named by a flag wins, no
//! output carries a secret, and no flag takes a secret as its value.

use std::fs;

use oxabl_cache_e2e::workdir::TestDir;

use crate::support::cli::{Output, run};
use crate::support::person::Person;
use crate::support::scenario::Owned;
use crate::support::sets;

/// A CI member that has no identity file: its identity string is a secret of the job.
struct CiJob {
    person: Person,
    identity: String,
    token: String,
}

impl CiJob {
    async fn join(owned: &Owned) -> Self {
        let mut person = Person::new("job");
        person.use_repo(owned.world.repo.path());
        let secret = person.file("identity.secret");
        let made = person
            .cli(&[
                "identity",
                "new",
                "--secret-out",
                &secret.display().to_string(),
            ])
            .ok()
            .json();
        let identity = fs::read_to_string(&secret)
            .expect("identity string")
            .trim()
            .to_owned();
        owned
            .owner
            .cli(&[
                "member",
                "add",
                made["descriptor"].as_str().expect("descriptor"),
                "--label",
                "job",
                "--role",
                "ci",
            ])
            .ok();
        let token = owned
            .world
            .token("job", &["read", "write:trusted"], &owned.cache)
            .await;
        Self {
            person,
            identity,
            token,
        }
    }

    /// Runs `args` with no secret flag: only the repository and the state directory.
    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut all: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        all.extend(["--json".to_owned()]);
        let repo = self.person.repo.as_ref().expect("repo");
        all.extend([
            "--repo".to_owned(),
            repo.display().to_string(),
            "--state-dir".to_owned(),
            self.person.state.display().to_string(),
        ]);
        run(self.person.dir.path(), &all, env)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ci_job_with_secrets_in_the_environment_publishes_a_dictionary() {
    let owned = Owned::start().await;
    let job = CiJob::join(&owned).await;
    let outputs = TestDir::new("outputs").expect("outputs");
    sets::write_outputs(outputs.path(), "train", 120, 8_000, true);
    let outputs_arg = outputs.path().display().to_string();
    let env = [
        ("OXABL_CACHE_TOKEN", job.token.as_str()),
        ("OXABL_CACHE_IDENTITY", job.identity.as_str()),
    ];

    let trained = job
        .run(
            &[
                "dict",
                "train",
                "--outputs",
                &outputs_arg,
                "--publish",
                "--mode",
                "ci",
            ],
            &env,
        )
        .ok();
    assert_eq!(trained.json()["published"]["pointer_version"], 1);
    let status = job.run(&["status", "--mode", "ci"], &env).ok().json();
    assert_eq!(status["principal"]["label"], "job");
    assert_eq!(status["server"]["dictionary_version"], 1);

    let without_token = job
        .run(
            &["status", "--mode", "ci"],
            &[("OXABL_CACHE_IDENTITY", job.identity.as_str())],
        )
        .exits(2);
    assert!(
        without_token.stderr.contains("no credentials"),
        "{}",
        without_token.stderr
    );
    job.run(
        &["status", "--mode", "ci"],
        &[("OXABL_CACHE_TOKEN", job.token.as_str())],
    )
    .exits(2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_passphrase_in_the_environment_unlocks_the_identity_file() {
    let owned = Owned::start().await;
    let owner = &owned.owner;
    let flags = [
        "--identity-file".to_owned(),
        owner.identity_arg(),
        "--token-file".to_owned(),
        owner
            .token_file
            .as_ref()
            .expect("token")
            .display()
            .to_string(),
        "--repo".to_owned(),
        owned.world.repo.path().display().to_string(),
        "--state-dir".to_owned(),
        owner.state.display().to_string(),
        "--json".to_owned(),
    ];
    let status = |env: &[(&str, &str)]| {
        let mut args = vec!["status".to_owned()];
        args.extend(flags.iter().cloned());
        run(owner.dir.path(), &args, env)
    };
    let unlocked = status(&[("OXABL_CACHE_IDENTITY_PASSPHRASE", &owner.passphrase)]).ok();
    assert_eq!(unlocked.json()["principal"]["label"], "owner");
    status(&[]).exits(2);
    let wrong = status(&[("OXABL_CACHE_IDENTITY_PASSPHRASE", "wrong-passphrase-value")]).exits(1);
    assert!(
        !wrong.everything().contains("wrong-passphrase-value"),
        "{wrong}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_named_by_a_flag_wins_over_the_environment() {
    let owned = Owned::start().await;
    let owner = &owned.owner;
    let stranger = CiJob::join(&owned).await;
    let bad_token = "token-that-would-be-refused";

    let with_files = |extra: &[(&str, &str)]| owner.cli_with(&["status"], extra);
    let token_wins = with_files(&[("OXABL_CACHE_TOKEN", bad_token)]).ok();
    assert_eq!(token_wins.json()["principal"]["label"], "owner");
    let passphrase_wins =
        with_files(&[("OXABL_CACHE_IDENTITY_PASSPHRASE", "not-the-passphrase")]).ok();
    assert_eq!(passphrase_wins.json()["principal"]["label"], "owner");
    let identity_wins = with_files(&[("OXABL_CACHE_IDENTITY", stranger.identity.as_str())]).ok();
    assert_eq!(
        identity_wins.json()["principal"]["label"],
        "owner",
        "the identity file, not the identity string in the environment, acts"
    );

    let args: Vec<String> = [
        "status",
        "--json",
        "--identity-file",
        "/nonexistent",
        "--passphrase-file",
        &owner.passphrase_file.display().to_string(),
        "--token-file",
        &owner
            .token_file
            .as_ref()
            .expect("token")
            .display()
            .to_string(),
        "--repo",
        &owned.world.repo.path().display().to_string(),
    ]
    .iter()
    .map(|a| (*a).to_owned())
    .collect();
    let missing_file = run(
        owner.dir.path(),
        &args,
        &[("OXABL_CACHE_IDENTITY", stranger.identity.as_str())],
    );
    assert_eq!(missing_file.code, 2, "{missing_file}");
    assert!(
        missing_file.stderr.contains("/nonexistent"),
        "a named file is never replaced by the environment: {missing_file}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_output_carries_a_secret_from_the_environment() {
    let owned = Owned::start().await;
    let job = CiJob::join(&owned).await;
    let bad_token = "tok-secret-value-0123456789";
    let bad_identity = "identity-secret-value-0123456789";
    let bad_passphrase = "passphrase-secret-value-0123456789";
    let work = TestDir::new("work").expect("work");
    let stage = work.path().display().to_string();
    let key = "ab".repeat(32);
    let good = [
        ("OXABL_CACHE_TOKEN", job.token.as_str()),
        ("OXABL_CACHE_IDENTITY", job.identity.as_str()),
    ];
    let mut printed = Vec::new();
    for env in [
        &good[..],
        &[
            ("OXABL_CACHE_TOKEN", bad_token),
            ("OXABL_CACHE_IDENTITY", job.identity.as_str()),
        ][..],
        &[
            ("OXABL_CACHE_TOKEN", job.token.as_str()),
            ("OXABL_CACHE_IDENTITY", bad_identity),
        ][..],
        &[
            ("OXABL_CACHE_TOKEN", bad_token),
            ("OXABL_CACHE_IDENTITY", bad_identity),
            ("OXABL_CACHE_IDENTITY_PASSPHRASE", bad_passphrase),
        ][..],
    ] {
        for args in [
            vec!["status", "--mode", "ci"],
            vec!["fetch", "--key", &key, "--stage", &stage, "--mode", "ci"],
            vec!["verify", "--sample", "3", "--mode", "ci"],
        ] {
            let ran = job.run(&args, env);
            printed.push(ran.everything());
            let mut text = args.clone();
            text.retain(|a| *a != "--json");
            let plain = {
                let repo = job
                    .person
                    .repo
                    .as_ref()
                    .expect("repo")
                    .display()
                    .to_string();
                let mut all: Vec<String> = text.iter().map(|a| (*a).to_owned()).collect();
                all.extend(["--repo".to_owned(), repo]);
                run(job.person.dir.path(), &all, env)
            };
            printed.push(plain.everything());
        }
    }
    let owner = &owned.owner;
    for (passphrase, code) in [(bad_passphrase, 1), (owner.passphrase.as_str(), 0)] {
        for json in [true, false] {
            let mut args: Vec<String> = ["status", "--identity-file", &owner.identity_arg()]
                .iter()
                .map(|a| (*a).to_owned())
                .collect();
            args.extend([
                "--token-file".to_owned(),
                owner
                    .token_file
                    .as_ref()
                    .expect("token")
                    .display()
                    .to_string(),
                "--repo".to_owned(),
                owned.world.repo.path().display().to_string(),
            ]);
            if json {
                args.push("--json".to_owned());
            }
            let ran = run(
                owner.dir.path(),
                &args,
                &[("OXABL_CACHE_IDENTITY_PASSPHRASE", passphrase)],
            );
            assert_eq!(ran.code, code, "{ran}");
            printed.push(ran.everything());
        }
    }
    let everything = printed.join("\n");
    for (what, secret) in [
        ("the token", job.token.as_str()),
        ("the identity string", job.identity.as_str()),
        ("a refused token", bad_token),
        ("a malformed identity string", bad_identity),
        ("a refused passphrase", bad_passphrase),
        ("a passphrase that unlocks", owner.passphrase.as_str()),
    ] {
        assert!(!everything.contains(secret), "{what} was printed");
    }
}

/// The `--option <VALUE>` pairs of one help text.
fn options(help: &str) -> Vec<(String, Option<String>)> {
    help.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let first = words.next()?;
            let long = if first.starts_with("--") {
                first
            } else if first.starts_with('-') && first.ends_with(',') {
                words.next().filter(|w| w.starts_with("--"))?
            } else {
                return None;
            };
            let value = words
                .next()
                .filter(|w| w.starts_with('<') && w.ends_with('>'))
                .map(str::to_owned);
            Some((long.to_owned(), value))
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_flag_takes_a_secret_as_its_value() {
    let person = Person::new("reader");
    let home = person.dir.path();
    let help = |path: &[&str]| {
        let mut args: Vec<String> = path.iter().map(|a| (*a).to_owned()).collect();
        args.push("--help".to_owned());
        run(home, &args, &[]).ok().stdout
    };
    let commands: Vec<Vec<&str>> = vec![
        vec![],
        vec!["identity", "new"],
        vec!["init"],
        vec!["member", "add"],
        vec!["member", "remove"],
        vec!["rotate"],
        vec!["owner", "add"],
        vec!["owner", "remove"],
        vec!["login"],
        vec!["logout"],
        vec!["status"],
        vec!["dict", "train"],
        vec!["verify"],
        vec!["fetch"],
        vec!["put"],
    ];
    let mut seen = 0;
    for path in &commands {
        for (flag, value) in options(&help(path)) {
            seen += 1;
            let secret_named = ["token", "passphrase", "secret", "password", "identity"]
                .iter()
                .any(|word| flag.contains(word));
            if secret_named && flag != "--help" {
                assert_eq!(
                    value.as_deref(),
                    Some("<PATH>"),
                    "{flag} of `{}` must name a file",
                    path.join(" ")
                );
            }
        }
    }
    assert!(seen > 40, "the help texts were read ({seen} options)");

    for flag in [
        "--token",
        "--passphrase",
        "--identity",
        "--identity-string",
        "--password",
        "--secret",
    ] {
        let args: Vec<String> = ["status", flag, "a-secret-value"]
            .iter()
            .map(|a| (*a).to_owned())
            .collect();
        let refused = run(home, &args, &[]).exits(2);
        assert!(
            refused.stderr.contains("unexpected argument"),
            "{flag}: {}",
            refused.stderr
        );
    }
}
