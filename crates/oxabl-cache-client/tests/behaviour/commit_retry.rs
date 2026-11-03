//! Retries: a lost commit reply, a failed pack write and a quota that is used up.

use oxabl_cache_client::{FetchOutcome, Mode, StoreFailure, StoreOutcome};

use crate::support::fixture;
use crate::support::proxy::CommitRule;
use crate::support::world::{World, assert_installed};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lost_commit_reply_is_retried_with_the_identical_body_and_still_reports_committed() {
    let world = World::start().await;
    world.proxy.commits(CommitRule::DropReplies(2));
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 5, "lost-reply");
    let report = ci.store(&sets).await;
    assert!(
        report
            .outcomes
            .iter()
            .all(|(_, o)| *o == StoreOutcome::Committed),
        "{:?}",
        report.outcomes
    );

    let commits: Vec<_> = world
        .proxy
        .seen()
        .into_iter()
        .filter(|s| s.path.ends_with("/commit"))
        .collect();
    assert_eq!(
        commits.len(),
        3,
        "two lost replies and the one that arrived"
    );
    assert!(
        commits.iter().all(|c| c.body == commits[0].body),
        "every retry carries the byte-identical body"
    );

    let reader = world.machine(&world.dev, Mode::Developer).await;
    let outcomes = reader.fetch(&sets).await;
    assert!(
        outcomes
            .iter()
            .all(|o| matches!(o, FetchOutcome::Installed { .. }))
    );
    assert_installed(&reader.stage(), &sets);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pack_closed_conflict_on_a_retry_never_reports_committed() {
    let world = World::start().await;
    world.proxy.commits(CommitRule::DropThenClose);
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 3, "closed");
    let report = ci.store(&sets).await;
    assert!(
        report
            .outcomes
            .iter()
            .all(|(_, o)| *o == StoreOutcome::Failed(StoreFailure::Unavailable)),
        "{:?}",
        report.outcomes
    );
    let commits = world
        .proxy
        .seen()
        .iter()
        .filter(|s| s.path.ends_with("/commit"))
        .count();
    assert_eq!(
        commits, 2,
        "the lost reply is retried once; a conflict is final"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_pack_write_is_retried_and_a_dead_store_is_unavailable() {
    let world = World::start().await;
    world.proxy.fail_puts(2);
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 4, "put-retry");
    let report = ci.store(&sets).await;
    assert!(
        report
            .outcomes
            .iter()
            .all(|(_, o)| *o == StoreOutcome::Committed),
        "{:?}",
        report.outcomes
    );
    let puts = world
        .proxy
        .seen()
        .iter()
        .filter(|s| s.method == "PUT" && s.path.contains("/objects/"))
        .count();
    assert_eq!(puts, 3, "two failures and the write that landed");

    world.proxy.fail_puts(100);
    let more = fixture::rekey(fixture::sets(&ci.workspace(), 2, "put-dead"), 50);
    let report = ci.store(&more).await;
    assert!(
        report
            .outcomes
            .iter()
            .all(|(_, o)| *o == StoreOutcome::Failed(StoreFailure::Unavailable)),
        "{:?}",
        report.outcomes
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writes_over_the_hard_quota_report_quota_exceeded_and_the_build_goes_on() {
    let world = World::start_with_quota(20_000).await;
    let dev = world.machine(&world.dev, Mode::Developer).await;
    let sets = fixture::sets(&dev.workspace(), 30, "quota");
    let report = dev.store(&sets).await;
    assert_eq!(report.outcomes.len(), 30);
    assert!(
        report
            .outcomes
            .iter()
            .all(|(_, o)| *o == StoreOutcome::Failed(StoreFailure::QuotaExceeded)),
        "{:?}",
        report.outcomes.first()
    );
    assert_eq!(report.bytes_uploaded, 0);
}
