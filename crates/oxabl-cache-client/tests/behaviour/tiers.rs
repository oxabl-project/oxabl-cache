//! Who may write which namespace, and whose entries a reader accepts.

use oxabl_cache_client::{CacheError, FetchOutcome, MissReason, Mode, StoreOutcome, Tier};
use oxabl_cache_format::{CommitStatus, Namespace};

use crate::support::fixture;
use crate::support::forge::Forgery;
use crate::support::world::{Member, World, assert_installed, identity_of, tree};

fn reasons(outcomes: &[FetchOutcome]) -> Vec<MissReason> {
    outcomes
        .iter()
        .map(|o| match o {
            FetchOutcome::Miss { reason, .. } => *reason,
            FetchOutcome::Installed { .. } => panic!("expected a miss, got {o:?}"),
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn developer_entries_are_personal_and_visible_to_their_author_only() {
    let world = World::start().await;
    let dev = world.machine(&world.dev, Mode::Developer).await;
    let sets = fixture::sets(&dev.workspace(), 8, "personal");
    let stored = dev.store(&sets).await;
    assert!(
        stored
            .outcomes
            .iter()
            .all(|(_, o)| *o == StoreOutcome::Committed)
    );

    let outcomes = dev.fetch(&sets).await;
    for outcome in &outcomes {
        let FetchOutcome::Installed { provenance, .. } = outcome else {
            panic!("the author must see their own entries: {outcome:?}");
        };
        assert_eq!(provenance.tier, Tier::Personal);
        assert_eq!(provenance.signer, world.dev.public);
    }
    assert_installed(&dev.stage(), &sets);

    for (member, mode) in [
        (&world.dev2, Mode::Developer),
        (&world.ci, Mode::Ci),
        (&world.reader, Mode::ReadOnly),
    ] {
        let other = world.machine(member, mode).await;
        let outcomes = other.fetch(&sets).await;
        assert_eq!(
            reasons(&outcomes),
            vec![MissReason::NotFound; 8],
            "{}",
            member.label
        );
        assert!(tree(&other.stage()).is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_developer_cannot_land_an_entry_in_trusted() {
    let world = World::start().await;
    let dev = world.machine(&world.dev, Mode::Developer).await;
    let sets = fixture::sets(&dev.workspace(), 4, "forged");

    assert_eq!(
        world
            .machine(&world.dev, Mode::Ci)
            .await
            .cache
            .uploader()
            .err(),
        Some(CacheError::NotAWriter),
        "a developer identity cannot write as CI"
    );
    assert_eq!(
        world
            .machine(&world.reader, Mode::ReadOnly)
            .await
            .cache
            .uploader()
            .err(),
        Some(CacheError::ReadOnly)
    );

    let developer = identity_of(&world.dev);
    let forgery = Forgery {
        world: &world,
        holder: &developer,
        signer: &developer,
        namespace: Namespace::Trusted,
        token: &world.ci.token,
        target: &world.cache,
        record_cache: oxabl_cache_format::CacheId::from_text(&world.cache).expect("id"),
        paths: None,
    };
    let statuses = forgery.publish(&sets).await;
    assert_eq!(
        statuses,
        vec![CommitStatus::Committed; 4],
        "the server cannot tell: it never opens a record"
    );

    for (member, mode) in [(&world.ci, Mode::Ci), (&world.dev2, Mode::Developer)] {
        let reader = world.machine(member, mode).await;
        let outcomes = reader.fetch(&sets).await;
        assert_eq!(reasons(&outcomes), vec![MissReason::Untrusted; 4]);
        assert!(tree(&reader.stage()).is_empty(), "nothing is installed");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ci_mode_never_reads_a_personal_entry_even_one_signed_by_its_own_key() {
    let world = World::start().await;
    let token = world
        .setup
        .token(
            &world.tenant_admin,
            "ci-both",
            &["read", "write:personal", "write:trusted"],
            &world.cache,
        )
        .await;
    let both = Member {
        label: "ci-both".into(),
        identity: world.ci.identity.clone(),
        token: token.clone(),
        public: world.ci.public,
    };
    let holder = identity_of(&world.ci);
    let scratch = world.machine(&both, Mode::Ci).await;
    let sets = fixture::sets(&scratch.workspace(), 3, "ci-personal");
    let forgery = Forgery {
        world: &world,
        holder: &holder,
        signer: &holder,
        namespace: Namespace::Personal,
        token: &token,
        target: &world.cache,
        record_cache: oxabl_cache_format::CacheId::from_text(&world.cache).expect("id"),
        paths: None,
    };
    assert_eq!(
        forgery.publish(&sets).await,
        vec![CommitStatus::Committed; 3]
    );

    assert_eq!(
        reasons(&scratch.fetch(&sets).await),
        vec![MissReason::NotFound; 3],
        "a ci client does not ask for its personal namespace"
    );
    let as_developer = world.machine(&both, Mode::Developer).await;
    assert!(
        as_developer
            .fetch(&sets)
            .await
            .iter()
            .all(|o| matches!(o, FetchOutcome::Installed { .. })),
        "the same entries are visible to a developer client"
    );
}
