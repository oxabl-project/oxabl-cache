//! The roster: rollback, rotation and a roster that is a version behind.

use oxabl_cache_client::{Cache, FetchOutcome, MissReason, Mode, OpenError};
use oxabl_cache_crypto::Identity;
use oxabl_cache_e2e::workdir::TestDir;
use oxabl_cache_format::Role;

use crate::support::fixture;
use crate::support::world::{World, tree};

fn reasons(outcomes: &[FetchOutcome]) -> Vec<Option<MissReason>> {
    outcomes
        .iter()
        .map(|o| match o {
            FetchOutcome::Miss { reason, .. } => Some(*reason),
            FetchOutcome::Installed { .. } => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rolled_back_roster_is_refused() {
    let mut world = World::start().await;
    let old_roster = world.roster.clone();
    let state = TestDir::new("state").expect("directory");
    let open = |world: &World| Cache::open(world.config(&world.dev, Mode::Developer, state.path()));
    open(&world).await.expect("version 1");

    world
        .add_member(
            "late",
            Role::Reader,
            &Identity::generate().expect("identity"),
        )
        .await;
    open(&world)
        .await
        .expect("version 2 raises the high-water mark");

    world.proxy.hold_roster(Some(old_roster.clone()));
    let refused = open(&world).await.err().expect("the old roster is refused");
    assert!(matches!(refused, OpenError::Roster(_)), "{refused:?}");

    let fresh = TestDir::new("state").expect("directory");
    world.write_repo_file(2);
    let refused = Cache::open(world.config(&world.dev, Mode::Developer, fresh.path()))
        .await
        .err()
        .expect("a runner without state relies on min_roster_version");
    assert!(matches!(refused, OpenError::Roster(_)), "{refused:?}");

    world.write_repo_file(0);
    Cache::open(world.config(&world.dev, Mode::Developer, fresh.path()))
        .await
        .expect("without a pin and without state, nothing says the roster is old");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_removed_member_cannot_read_the_new_epoch() {
    let mut world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let before = fixture::sets(&ci.workspace(), 6, "epoch-one");
    ci.store(&before).await;
    let doomed = world.machine(&world.dev2, Mode::Developer).await;
    assert!(
        reasons(&doomed.fetch(&before).await)
            .iter()
            .all(Option::is_none),
        "while a member, the entries are readable"
    );

    world.remove_member("dev2").await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let after = fixture::rekey(fixture::sets(&ci.workspace(), 6, "epoch-two"), 100);
    ci.store(&after).await;

    let stale = doomed.fetch(&after).await;
    assert_eq!(reasons(&stale), vec![Some(MissReason::NotFound); 6]);
    assert!(
        tree(&doomed.stage()).keys().all(|name| before
            .iter()
            .flat_map(|s| &s.files)
            .any(|f| &f.path == name)),
        "nothing of the new epoch reached the stage"
    );
    let reopened = world
        .open(&world.dev2, Mode::Developer)
        .await
        .err()
        .expect("refused");
    assert_eq!(reopened, OpenError::IdentityNotInRoster);

    let kept = world.machine(&world.dev, Mode::Developer).await;
    assert!(
        reasons(&kept.fetch(&after).await)
            .iter()
            .all(Option::is_none)
    );
    assert_eq!(
        reasons(&kept.fetch(&before).await),
        vec![Some(MissReason::NotFound); 6],
        "a rotation starts cold"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_record_ahead_of_the_roster_refetches_it_once_and_then_installs() {
    let mut world = World::start().await;
    let stale = world.machine(&world.dev, Mode::Developer).await;
    world
        .add_member(
            "late",
            Role::Reader,
            &Identity::generate().expect("identity"),
        )
        .await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 5, "ahead");
    ci.store(&sets).await;

    let outcomes = stale.fetch(&sets).await;
    assert!(
        reasons(&outcomes).iter().all(Option::is_none),
        "{outcomes:?}"
    );
    let roster_reads = world
        .proxy
        .seen()
        .iter()
        .filter(|s| s.path.ends_with("/roster"))
        .count();
    assert_eq!(roster_reads, 3, "one open each and exactly one refetch");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_withheld_roster_leaves_records_that_are_ahead_as_misses() {
    let mut world = World::start().await;
    let old_roster = world.roster.clone();
    let stale = world.machine(&world.dev, Mode::Developer).await;
    world
        .add_member(
            "late",
            Role::Reader,
            &Identity::generate().expect("identity"),
        )
        .await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 5, "withheld");
    ci.store(&sets).await;

    world.proxy.hold_roster(Some(old_roster));
    let outcomes = stale.fetch(&sets).await;
    assert_eq!(reasons(&outcomes), vec![Some(MissReason::Untrusted); 5]);
    assert!(tree(&stale.stage()).is_empty());
}
