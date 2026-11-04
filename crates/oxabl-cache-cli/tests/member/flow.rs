//! An owner runs a cache from its first roster to a rotation that purges the old epochs, and
//! the client library, as oxabl uses it, reads and writes with each member's own identity.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use oxabl_cache_client::{FetchOutcome, Mode, OpenError, Presence, Tier};
use oxabl_cache_e2e::workdir::TestDir;

use crate::support::cache::{Member, assert_all_missed, assert_installed, fetch, store};
use crate::support::person::Person;
use crate::support::sets;
use crate::support::world::World;

fn tier_of(outcome: &FetchOutcome) -> Tier {
    match outcome {
        FetchOutcome::Installed { provenance, .. } => provenance.tier,
        other => panic!("not installed: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_owner_takes_a_cache_from_init_through_a_removal_to_a_purging_rotation() {
    let world = World::start().await;
    let mut owner = Person::new("owner");
    owner.use_repo(world.repo.path());

    let made = owner
        .cli(&["identity", "new", "--file", &owner.identity_arg()])
        .ok()
        .json();
    let cache = world
        .create_cache(
            "codebase",
            made["signing_key_hex"].as_str().expect("owner key"),
        )
        .await;
    owner.use_token(&world.token("owner", &["read", "cache:admin"], &cache).await);

    let init = owner
        .cli(&[
            "init",
            "--server",
            &world.url(),
            "--tenant",
            &world.tenant,
            "--cache",
            &cache,
            "--label",
            "owner",
        ])
        .ok()
        .json();
    assert_eq!(
        (init["roster_version"].as_u64(), init["epoch"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(init["owner_key"], made["signing_key"]);
    world.write_repo_file(init["cache_toml"].as_str().expect("cache.toml"));

    let ci = Person::new("ci");
    let ci_secret = ci.file("identity.secret");
    let ci_made = ci
        .cli(&[
            "identity",
            "new",
            "--secret-out",
            &ci_secret.display().to_string(),
        ])
        .ok();
    assert_eq!(
        fs::metadata(&ci_secret)
            .expect("secret")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let ci_identity = fs::read_to_string(&ci_secret)
        .expect("secret")
        .trim()
        .to_owned();
    assert!(
        !ci_made.everything().contains(&ci_identity),
        "the identity string goes to its file and is never printed"
    );
    let dev = Person::new("dev");
    let dev_made = dev
        .cli(&["identity", "new", "--file", &dev.identity_arg()])
        .ok()
        .json();

    let ci_member = Member::with_secret(
        &world.token("ci", &["read", "write:trusted"], &cache).await,
        &ci_identity,
    );
    let dev_member = Member::with_file(
        &world
            .token("dev", &["read", "write:personal"], &cache)
            .await,
        &dev.identity,
        &dev.passphrase,
    );

    for (descriptor, label, role, version) in [
        (&ci_made.json(), "ci", "ci", 2),
        (&dev_made, "dev", "developer", 3),
    ] {
        let added = owner
            .cli(&[
                "member",
                "add",
                descriptor["descriptor"].as_str().expect("descriptor"),
                "--label",
                label,
                "--role",
                role,
            ])
            .ok()
            .json();
        assert_eq!(added["roster_version"], version);
        assert_eq!(added["epoch"], 1, "a new member joins the same epoch");
    }

    let work = TestDir::new("work").expect("work");
    let (ci_state, dev_state) = (work.path().join("ci-state"), work.path().join("dev-state"));
    let ci_cache = ci_member
        .open(world.repo.path(), Mode::Ci, &ci_state)
        .await
        .expect("ci opens");
    let dev_cache = dev_member
        .open(world.repo.path(), Mode::Developer, &dev_state)
        .await
        .expect("dev opens");
    let built = sets::make(&work.path().join("ci-workspace"), "a", 3);
    store(&ci_cache, &built).await;
    let fetched = fetch(&dev_cache, &built, &work.path().join("dev-stage")).await;
    assert_installed(&fetched, &built, &work.path().join("dev-stage"));
    assert!(fetched.iter().all(|o| tier_of(o) == Tier::Trusted));

    let own = sets::make(&work.path().join("dev-workspace"), "mine", 1);
    store(&dev_cache, &own).await;
    let personal = fetch(&dev_cache, &own, &work.path().join("dev-stage-own")).await;
    assert_installed(&personal, &own, &work.path().join("dev-stage-own"));
    assert_eq!(tier_of(&personal[0]), Tier::Personal);
    assert_all_missed(&fetch(&ci_cache, &own, &work.path().join("ci-stage-own")).await);

    let status = owner.cli(&["status"]).ok().json();
    assert_eq!(status["on"], true);
    assert_eq!(status["principal"]["label"], "owner");
    assert_eq!(
        status["roster"]["members"].as_array().map(Vec::len),
        Some(3)
    );
    assert_eq!(status["server"]["epoch"], 1);

    let removed = owner.cli(&["member", "remove", "dev"]).ok().json();
    assert_eq!(
        (
            removed["roster_version"].as_u64(),
            removed["epoch"].as_u64()
        ),
        (Some(4), Some(2))
    );
    assert_eq!(
        removed["min_roster_version"], 4,
        "the new pin the repository must carry"
    );

    assert!(
        matches!(
            dev_member
                .open(
                    world.repo.path(),
                    Mode::Developer,
                    &work.path().join("dev-state-2")
                )
                .await,
            Err(OpenError::IdentityNotInRoster)
        ),
        "the removed member cannot open the cache on the new roster"
    );
    let ci_cache = ci_member
        .open(world.repo.path(), Mode::Ci, &ci_state)
        .await
        .expect("ci opens");
    assert_all_missed(&fetch(&ci_cache, &built, &work.path().join("cold-stage")).await);
    let rebuilt = sets::make(&work.path().join("ci-workspace-2"), "b", 3);
    store(&ci_cache, &rebuilt).await;
    let hot = fetch(&ci_cache, &rebuilt, &work.path().join("hot-stage")).await;
    assert_installed(&hot, &rebuilt, &work.path().join("hot-stage"));
    assert_all_missed(&fetch(&dev_cache, &rebuilt, &work.path().join("locked-out")).await);

    let rotated = owner.cli(&["rotate"]).ok().json();
    assert_eq!(rotated["epoch"], 3);
    assert_installed(
        &fetch(&ci_cache, &rebuilt, &work.path().join("after-rotate")).await,
        &rebuilt,
        &work.path().join("after-rotate"),
    );
    let at_three = ci_member
        .open(world.repo.path(), Mode::Ci, &ci_state)
        .await
        .expect("ci opens");
    let latest = sets::make(&work.path().join("ci-workspace-3"), "c", 3);
    store(&at_three, &latest).await;
    assert_installed(
        &fetch(&at_three, &latest, &work.path().join("epoch-three")).await,
        &latest,
        &work.path().join("epoch-three"),
    );

    let purged = owner.cli(&["rotate", "--purge"]).ok().json();
    assert_eq!(purged["epoch"], 4);
    assert!(
        purged["purge"]["entries_marked"].as_u64() >= Some((rebuilt.len() + latest.len()) as u64),
        "{purged}"
    );
    for (handle, old) in [(&at_three, &latest), (&ci_cache, &rebuilt)] {
        assert_all_missed(&fetch(handle, old, &work.path().join("after-purge")).await);
        assert!(
            handle
                .probe(&old.iter().map(|s| s.key).collect::<Vec<_>>())
                .await
                .expect("probe")
                .iter()
                .all(|p| *p == Presence::Miss),
            "nothing is readable under an old epoch"
        );
    }
    assert_all_missed(&fetch(&dev_cache, &built, &work.path().join("old-stage")).await);

    let renewed = ci_member
        .open(world.repo.path(), Mode::Ci, &ci_state)
        .await
        .expect("ci opens");
    store(&renewed, &latest).await;
    assert_installed(
        &fetch(&renewed, &latest, &work.path().join("renewed-stage")).await,
        &latest,
        &work.path().join("renewed-stage"),
    );
}
