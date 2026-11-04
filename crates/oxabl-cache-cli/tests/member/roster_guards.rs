//! Who may change the roster: only an owner whose key the repository pins and whose token
//! carries `cache:admin`, never the only owner away from itself, and never over a newer roster.

use oxabl_cache_client::{Config, Mode, Owner, OwnerError, Role};

use crate::support::cache::Member;
use crate::support::person::Person;
use crate::support::scenario::Owned;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_only_owner_cannot_remove_itself() {
    let owned = Owned::start().await;
    owned.owner.cli(&["owner", "remove", "owner"]).exits(1);
    let status = owned.owner.cli(&["status"]).ok().json();
    assert_eq!(status["roster"]["version"], 1);
    assert_eq!(
        status["roster"]["members"].as_array().map(Vec::len),
        Some(1)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_an_owner_with_cache_admin_changes_the_roster_and_a_stale_change_is_refused() {
    let owned = Owned::start().await;
    let dev = owned
        .member("dev", "developer", &["read", "write:personal"])
        .await;
    let refused = dev
        .cli(&["member", "add", "garbage", "--label", "x", "--role", "ci"])
        .exits(1);
    assert!(
        refused.stderr.contains("not an owner"),
        "{}",
        refused.stderr
    );

    let mut bare = Person::new("bare");
    bare.use_repo(owned.world.repo.path());
    bare.identity = owned.owner.identity.clone();
    bare.passphrase_file = owned.owner.passphrase_file.clone();
    bare.use_token(
        &owned
            .world
            .token("read-only", &["read"], &owned.cache)
            .await,
    );
    let no_admin = bare.cli(&["rotate"]).exits(1);
    assert!(
        no_admin.stderr.contains("cache:admin"),
        "{}",
        no_admin.stderr
    );

    let open = |state: &str| {
        let owner = &owned.owner;
        let config: Config = Member::with_file(&owner.token, &owner.identity, &owner.passphrase)
            .config(
                owned.world.repo.path(),
                Mode::Developer,
                &owned.owner.dir.path().join(state),
            );
        Owner::open(config)
    };
    let (first, second) = (
        open("first").await.expect("owner"),
        open("second").await.expect("owner"),
    );
    let carol = Person::new("carol");
    let carol_made = carol
        .cli(&["identity", "new", "--file", &carol.identity_arg()])
        .ok()
        .json();
    first
        .add_member(
            carol_made["descriptor"].as_str().expect("descriptor"),
            "carol",
            Role::Reader,
        )
        .await
        .expect("the first change publishes");
    let stale = second
        .add_member(
            carol_made["descriptor"].as_str().expect("descriptor"),
            "carol-again",
            Role::Reader,
        )
        .await;
    assert_eq!(stale.map(|p| p.roster_version), Err(OwnerError::Stale));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn init_refuses_a_cache_that_already_has_a_roster() {
    let owned = Owned::start().await;
    let again = owned
        .owner
        .cli(&[
            "init",
            "--server",
            &owned.world.url(),
            "--tenant",
            &owned.world.tenant,
            "--cache",
            &owned.cache,
            "--label",
            "owner",
        ])
        .exits(1);
    assert!(
        again.stderr.contains("already has a roster"),
        "{}",
        again.stderr
    );
    let status = owned.owner.cli(&["status"]).ok().json();
    assert_eq!(status["roster"]["version"], 1);
}
