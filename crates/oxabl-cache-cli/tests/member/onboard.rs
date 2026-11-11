//! `onboard ci` and the `token` commands: a tenant administrator who owns the cache connects a
//! CI system in one command, and a failure never leaves a half-done state behind.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use oxabl_cache_client::Mode;
use oxabl_cache_e2e::workdir::TestDir;
use serde_json::Value;

use crate::support::cache::{Member, assert_installed, fetch, store};
use crate::support::cli::{Output, run};
use crate::support::flaky::{Fault, Flaky};
use crate::support::person::{Person, write_secret};
use crate::support::scenario::Owned;
use crate::support::sets;

/// An owner who also holds the tenant's administrator token.
struct Admin {
    owned: Owned,
    admin_token: String,
    out: TestDir,
}

impl Admin {
    async fn start() -> Self {
        let owned = Owned::start().await;
        let admin_token = owned.owner.file("admin-token");
        write_secret(&admin_token, &owned.world.tenant_admin);
        Self {
            admin_token: admin_token.display().to_string(),
            out: TestDir::new("onboard-out").expect("output directory"),
            owned,
        }
    }

    fn out(&self) -> String {
        self.out.path().display().to_string()
    }

    fn onboard(&self, person: &Person, extra: &[&str]) -> Output {
        self.onboard_with(person, extra, &[])
    }

    fn onboard_with(&self, person: &Person, extra: &[&str], env: &[(&str, &str)]) -> Output {
        let out = self.out();
        let mut args = vec![
            "onboard",
            "ci",
            "--admin-token-file",
            &self.admin_token,
            "--out",
            &out,
        ];
        args.extend_from_slice(extra);
        person.cli_with(&args, env)
    }

    fn tokens(&self) -> Vec<Value> {
        let listed = self
            .owned
            .owner
            .cli(&["token", "list", "--admin-token-file", &self.admin_token])
            .ok()
            .json();
        listed["tokens"].as_array().expect("tokens").clone()
    }

    fn tokens_named(&self, name: &str) -> Vec<Value> {
        self.tokens()
            .into_iter()
            .filter(|t| t["name"] == name)
            .collect()
    }

    fn members(&self) -> usize {
        let status = self.owned.owner.cli(&["status"]).ok().json();
        status["roster"]["members"]
            .as_array()
            .expect("members")
            .len()
    }

    fn roster_version(&self) -> u64 {
        self.owned.owner.cli(&["status"]).ok().json()["roster"]["version"]
            .as_u64()
            .expect("version")
    }

    /// Nothing of an onboarding is left: no member, no live token of that name, no file.
    fn assert_untouched(&self, name: &str, members: usize) {
        assert_eq!(self.members(), members, "the roster is as it was");
        assert_eq!(self.roster_version(), members as u64);
        assert!(
            self.tokens_named(name)
                .iter()
                .all(|t| !t["revoked_at"].is_null()),
            "no live token named {name}"
        );
        assert!(
            fs::read_dir(self.out.path()).map_or(true, |mut d| d.next().is_none()),
            "no file in the output directory"
        );
    }
}

fn secret_of(path: &Path) -> String {
    assert_eq!(
        fs::metadata(path)
            .expect("secret file")
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "{}",
        path.display()
    );
    fs::read_to_string(path).expect("secret").trim().to_owned()
}

/// Runs the printed check line as the CI job would: the two secrets in the environment only.
fn run_check(admin: &Admin, check: &str, identity: &str, token: &str) -> Output {
    let home = TestDir::new("ci-home").expect("home");
    let mut args: Vec<String> = check
        .split_whitespace()
        .skip(1)
        .map(str::to_owned)
        .collect();
    args.extend([
        "--json".to_owned(),
        "--repo".to_owned(),
        admin.owned.world.repo.path().display().to_string(),
    ]);
    run(
        home.path(),
        &args,
        &[
            ("OXABL_CACHE_IDENTITY", identity),
            ("OXABL_CACHE_TOKEN", token),
        ],
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_command_connects_a_ci_system_that_then_writes_trusted_entries() {
    let admin = Admin::start().await;
    let done = admin
        .onboard(&admin.owned.owner, &["--name", "pipeline"])
        .ok();
    let result = done.json();
    let identity = secret_of(&admin.out.path().join("OXABL_CACHE_IDENTITY"));
    let token = secret_of(&admin.out.path().join("OXABL_CACHE_TOKEN"));
    assert!(
        !done.everything().contains(&identity) && !done.everything().contains(&token),
        "the secrets go to their files and are never printed:\n{done}"
    );
    assert_eq!(result["member"]["role"], "ci");
    assert_eq!(result["member"]["roster_version"], 2);
    assert_eq!(
        result["token"]["scopes"],
        serde_json::json!(["read", "write:trusted"])
    );
    assert_eq!(
        result["secrets"]["OXABL_CACHE_TOKEN"],
        admin.out().to_string() + "/OXABL_CACHE_TOKEN"
    );

    let check = result["check"].as_str().expect("check line");
    let status = run_check(&admin, check, &identity, &token).ok().json();
    assert_eq!(status["principal"]["label"], "pipeline");
    assert_eq!(status["principal"]["role"], "ci");
    assert_eq!(status["mode"], "ci");

    let work = TestDir::new("work").expect("work");
    let ci = Member::with_secret(&token, &identity);
    let cache = ci
        .open(
            admin.owned.world.repo.path(),
            Mode::Ci,
            &work.path().join("state"),
        )
        .await
        .expect("the onboarded system opens the cache");
    let built = sets::make(&work.path().join("workspace"), "a", 2);
    store(&cache, &built).await;
    let fetched = fetch(&cache, &built, &work.path().join("stage")).await;
    assert_installed(&fetched, &built, &work.path().join("stage"));

    let listed = admin.tokens_named("pipeline");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], result["token"]["id"]);
    assert_eq!(
        listed[0]["grants"][0]["cache"], admin.owned.cache,
        "the token is for this cache only"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reader_system_gets_read_only_scopes_and_cannot_write() {
    let admin = Admin::start().await;
    let done = admin
        .onboard(
            &admin.owned.owner,
            &[
                "--name",
                "viewer",
                "--role",
                "reader",
                "--expires-in-days",
                "7",
            ],
        )
        .ok();
    let result = done.json();
    assert_eq!(result["member"]["role"], "reader");
    assert_eq!(result["token"]["scopes"], serde_json::json!(["read"]));
    let identity = secret_of(&admin.out.path().join("OXABL_CACHE_IDENTITY"));
    let token = secret_of(&admin.out.path().join("OXABL_CACHE_TOKEN"));
    let status = run_check(
        &admin,
        result["check"].as_str().expect("check"),
        &identity,
        &token,
    )
    .ok()
    .json();
    assert_eq!(status["principal"]["role"], "reader");
    assert_eq!(status["mode"], "read-only");

    let work = TestDir::new("work").expect("work");
    let reader = Member::with_secret(&token, &identity);
    let cache = reader
        .open(
            admin.owned.world.repo.path(),
            Mode::Ci,
            &work.path().join("state"),
        )
        .await
        .expect("opens");
    assert!(
        cache.uploader().is_err(),
        "a reader member is never a writer"
    );
    let expires = admin.tokens_named("viewer")[0]["expires_at"]
        .as_i64()
        .expect("expiry");
    let created = admin.tokens_named("viewer")[0]["created_at"]
        .as_i64()
        .expect("creation");
    assert_eq!(expires - created, 7 * 86_400);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_existing_output_file_is_refused_before_anything_changes() {
    let admin = Admin::start().await;
    let existing = admin.out.path().join("OXABL_CACHE_TOKEN");
    fs::write(&existing, "kept\n").expect("existing file");
    let refused = admin
        .onboard(&admin.owned.owner, &["--name", "pipeline"])
        .exits(1);
    assert!(
        refused.stderr.contains("OXABL_CACHE_TOKEN") && refused.stderr.contains("exists"),
        "{refused}"
    );
    assert_eq!(fs::read_to_string(&existing).expect("kept"), "kept\n");
    assert!(!admin.out.path().join("OXABL_CACHE_IDENTITY").exists());
    assert_eq!(admin.members(), 1);
    assert!(
        admin.tokens_named("pipeline").is_empty(),
        "no token was made"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_member_that_is_not_an_owner_is_refused_before_anything_changes() {
    let admin = Admin::start().await;
    let dev = admin
        .owned
        .member(
            "dev",
            "developer",
            &["read", "write:personal", "cache:admin"],
        )
        .await;
    let refused = admin.onboard(&dev, &["--name", "pipeline"]).exits(1);
    assert!(refused.stderr.contains("not an owner"), "{refused}");
    admin.assert_untouched("pipeline", 2);
    assert!(
        admin.tokens_named("pipeline").is_empty(),
        "no token was made"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_passphrase_is_named_as_such() {
    let admin = Admin::start().await;
    let mut wrong = Person::new("wrong");
    wrong.use_repo(admin.owned.world.repo.path());
    wrong.identity = admin.owned.owner.identity.clone();
    wrong.state = admin.owned.owner.state.clone();
    write_secret(&wrong.passphrase_file, "not-the-owners-passphrase");
    wrong.use_token(&admin.owned.owner.token);
    let refused = admin.onboard(&wrong, &["--name", "pipeline"]).exits(1);
    assert!(
        refused.stderr.contains("passphrase does not unlock"),
        "{refused}"
    );
    assert!(admin.tokens_named("pipeline").is_empty());
    assert_eq!(admin.members(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_admin_token_without_tenant_admin_is_refused() {
    let admin = Admin::start().await;
    let weak = admin.owned.owner.file("weak-token");
    write_secret(&weak, &admin.owned.owner.token);
    let out = admin.out();
    let refused = admin.owned.owner.cli(&[
        "onboard",
        "ci",
        "--name",
        "pipeline",
        "--admin-token-file",
        &weak.display().to_string(),
        "--out",
        &out,
    ]);
    let refused = refused.exits(1);
    assert!(
        refused.stderr.contains("does not carry tenant:admin"),
        "{refused}"
    );
    admin.assert_untouched("pipeline", 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_label_that_is_taken_is_refused_before_a_token_is_made() {
    let admin = Admin::start().await;
    let before = admin.tokens().len();
    let refused = admin
        .onboard(&admin.owned.owner, &["--name", "owner"])
        .exits(2);
    assert!(refused.stderr.contains("label exists"), "{refused}");
    assert_eq!(admin.tokens().len(), before, "no token was made");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_token_the_server_refuses_leaves_no_member_no_token_and_no_file() {
    let admin = Admin::start().await;
    let refused = admin
        .onboard(
            &admin.owned.owner,
            &["--name", "pipeline", "--expires-in-days", "4000"],
        )
        .exits(1);
    assert!(refused.stderr.contains("out of range"), "{refused}");
    admin.assert_untouched("pipeline", 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_secret_file_that_cannot_be_written_revokes_the_token_it_just_made() {
    let admin = Admin::start().await;
    let out = "/proc/oxabl-cache-no-such-directory/secrets";
    let refused = admin
        .owned
        .owner
        .cli(&[
            "onboard",
            "ci",
            "--name",
            "pipeline",
            "--admin-token-file",
            &admin.admin_token,
            "--out",
            out,
        ])
        .exits(1);
    assert!(refused.stderr.contains("was revoked again"), "{refused}");
    let tokens = admin.tokens_named("pipeline");
    assert_eq!(tokens.len(), 1, "the token existed for a moment");
    assert!(
        !tokens[0]["revoked_at"].is_null(),
        "and is revoked: {tokens:?}"
    );
    assert_eq!(admin.members(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_roster_the_server_refuses_takes_the_token_and_the_files_back() {
    let admin = Admin::start().await;
    let other = Person::new("other");
    let made = other
        .cli(&["identity", "new", "--file", &other.identity_arg()])
        .ok()
        .json();
    admin
        .owned
        .world
        .set_owner_keys(
            &admin.owned.cache,
            &[made["signing_key_hex"].as_str().expect("key").to_owned()],
        )
        .await;
    let refused = admin
        .onboard(&admin.owned.owner, &["--name", "pipeline"])
        .exits(1);
    assert!(
        refused.stderr.contains("the roster is unchanged")
            && refused
                .stderr
                .contains("is revoked and the files are removed"),
        "{refused}"
    );
    let tokens = admin.tokens_named("pipeline");
    assert_eq!(tokens.len(), 1);
    assert!(!tokens[0]["revoked_at"].is_null(), "{tokens:?}");
    assert!(
        fs::read_dir(admin.out.path())
            .expect("dir")
            .next()
            .is_none(),
        "the secret files are gone"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tokens_are_created_listed_and_revoked_from_the_command_line() {
    let admin = Admin::start().await;
    let owner = &admin.owned.owner;
    let file = admin.out.path().join("scanner-token");
    let file_arg = file.display().to_string();
    let created = owner
        .cli(&[
            "token",
            "create",
            "--admin-token-file",
            &admin.admin_token,
            "--scopes",
            "read,write:personal",
            "--name",
            "scanner",
            "--out",
            &file_arg,
        ])
        .ok();
    let token = secret_of(&file);
    assert!(!created.everything().contains(&token), "{created}");
    let id = created.json()["id"].as_str().expect("id").to_owned();

    owner
        .cli(&[
            "token",
            "create",
            "--admin-token-file",
            &admin.admin_token,
            "--scopes",
            "read",
            "--name",
            "again",
            "--out",
            &file_arg,
        ])
        .exits(1);
    assert_eq!(
        secret_of(&file),
        token,
        "an existing file is never overwritten"
    );

    let listed = admin.tokens_named("scanner");
    assert_eq!(listed[0]["id"], id.as_str());
    assert_eq!(
        listed[0]["grants"][0]["scopes"],
        serde_json::json!(["read", "write:personal"])
    );
    let text = owner.text(&["token", "list", "--admin-token-file", &admin.admin_token]);
    assert!(text.stdout.contains("scanner"), "{text}");
    assert!(!text.stdout.contains(&token), "{text}");

    let probe = Person::new("probe");
    let mut probe = probe;
    probe.use_repo(admin.owned.world.repo.path());
    probe.use_token(&token);
    probe.identity = owner.identity.clone();
    probe.passphrase_file = owner.passphrase_file.clone();
    probe.cli(&["status"]).ok();
    owner
        .cli(&[
            "token",
            "revoke",
            &id,
            "--admin-token-file",
            &admin.admin_token,
        ])
        .ok();
    probe.cli(&["status"]).exits(1);
    assert!(!admin.tokens_named("scanner")[0]["revoked_at"].is_null());
}

fn flaky(admin: &Admin, fault: Fault) -> Flaky {
    Flaky::start(
        admin.owned.world.server.public.parse().expect("address"),
        fault,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lost_answer_to_the_roster_change_is_settled_by_reading_the_roster() {
    let admin = Admin::start().await;
    let proxy = flaky(&admin, Fault::LoseAnswer);
    let told = admin
        .onboard_with(
            &admin.owned.owner,
            &["--name", "pipeline"],
            &[("OXABL_CACHE_SERVER", &proxy.url())],
        )
        .exits(1);
    assert!(
        told.stderr
            .contains("holds the member pipeline nevertheless"),
        "{told}"
    );
    assert_eq!(admin.members(), 2, "the server stored the roster");
    let tokens = admin.tokens_named("pipeline");
    assert!(
        tokens[0]["revoked_at"].is_null(),
        "the token stays: {tokens:?}"
    );
    let identity = secret_of(&admin.out.path().join("OXABL_CACHE_IDENTITY"));
    let token = secret_of(&admin.out.path().join("OXABL_CACHE_TOKEN"));
    run_check(&admin, "oxabl-cache --mode ci status", &identity, &token).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_roster_change_that_never_arrived_is_undone() {
    let admin = Admin::start().await;
    let proxy = flaky(&admin, Fault::LoseRequest);
    let told = admin
        .onboard_with(
            &admin.owned.owner,
            &["--name", "pipeline"],
            &[("OXABL_CACHE_SERVER", &proxy.url())],
        )
        .exits(1);
    assert!(
        told.stderr.contains("the roster is unchanged")
            && told.stderr.contains("is revoked and the files are removed"),
        "{told}"
    );
    admin.assert_untouched("pipeline", 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_roster_change_whose_outcome_cannot_be_read_says_what_to_check_and_undo() {
    let admin = Admin::start().await;
    let proxy = flaky(&admin, Fault::LoseRequestAndGoDark);
    let told = admin
        .onboard_with(
            &admin.owned.owner,
            &["--name", "pipeline"],
            &[("OXABL_CACHE_SERVER", &proxy.url())],
        )
        .exits(1);
    assert!(
        told.stderr.contains("not known whether the roster holds")
            && told.stderr.contains("token revoke")
            && told.stderr.contains("oxabl-cache status"),
        "{told}"
    );
    let tokens = admin.tokens_named("pipeline");
    assert!(
        tokens[0]["revoked_at"].is_null(),
        "nothing is undone blindly"
    );
    assert!(admin.out.path().join("OXABL_CACHE_TOKEN").exists());
    assert_eq!(admin.members(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_owner_token_without_cache_admin_is_refused_before_a_token_is_made() {
    let admin = Admin::start().await;
    let mut weak = Person::new("weak");
    weak.use_repo(admin.owned.world.repo.path());
    weak.identity = admin.owned.owner.identity.clone();
    weak.passphrase_file = admin.owned.owner.passphrase_file.clone();
    weak.use_token(
        &admin
            .owned
            .world
            .token("read-only", &["read"], &admin.owned.cache)
            .await,
    );
    let before = admin.tokens().len();
    let refused = admin.onboard(&weak, &["--name", "pipeline"]).exits(1);
    assert!(refused.stderr.contains("no cache:admin"), "{refused}");
    assert_eq!(admin.tokens().len(), before, "no token was made");
    admin.assert_untouched("pipeline", 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_admin_token_of_another_tenant_is_refused() {
    let admin = Admin::start().await;
    let created = admin
        .owned
        .world
        .server
        .admin_cli(&["--json", "tenant", "create", "elsewhere"]);
    let other: Value = serde_json::from_slice(&created.stdout).expect("tenant");
    let token = admin.owned.world.server.admin_cli(&[
        "--json",
        "token",
        "create",
        "--tenant",
        other["id"].as_str().expect("tenant id"),
        "--scopes",
        "tenant:admin",
    ]);
    let token: Value = serde_json::from_slice(&token.stdout).expect("token");
    let file = admin.owned.owner.file("other-admin-token");
    write_secret(&file, token["token"].as_str().expect("token"));
    let out = admin.out();
    let refused = admin
        .owned
        .owner
        .cli(&[
            "onboard",
            "ci",
            "--name",
            "pipeline",
            "--admin-token-file",
            &file.display().to_string(),
            "--out",
            &out,
        ])
        .exits(1);
    assert!(refused.stderr.contains("another tenant"), "{refused}");
    admin.assert_untouched("pipeline", 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_token_with_other_scopes_than_the_role_needs_is_revoked() {
    let admin = Admin::start().await;
    let proxy = flaky(&admin, Fault::WidenScopes);
    let refused = admin
        .onboard_with(
            &admin.owned.owner,
            &["--name", "pipeline"],
            &[("OXABL_CACHE_SERVER", &proxy.url())],
        )
        .exits(1);
    assert!(
        refused
            .stderr
            .contains("other scopes than the role ci needs")
            && refused.stderr.contains("the roster is unchanged"),
        "{refused}"
    );
    admin.assert_untouched("pipeline", 1);
    let tokens = admin.tokens_named("pipeline");
    assert_eq!(tokens.len(), 1);
    assert!(!tokens[0]["revoked_at"].is_null(), "{tokens:?}");
}
