//! Reports are verified by the server's own re-hash and rate limited per principal.

use crate::support::{Env, entry};
use oxabl_cache_core::Error;
use oxabl_cache_core::domain::namespace::NamespaceKind::Trusted;
use oxabl_cache_core::ids::{CacheId, PackId, SegmentId};
use oxabl_cache_core::usecases::ReportOutcome;

async fn three_entries(env: &Env) -> (PackId, crate::support::Built) {
    let built = env.artifacts(&[1, 2, 3], 4096);
    let (pack, _) = env
        .publish(
            &env.ci,
            Trusted,
            &built,
            vec![entry(1, 0), entry(2, 1), entry(3, 2)],
        )
        .await;
    (pack, built)
}

fn damage(env: &Env, pack: PackId, offset: u64) {
    let path = env.object_path(pack);
    let mut bytes = std::fs::read(&path).expect("read");
    bytes[usize::try_from(offset).expect("offset") + 7] ^= 0xa5;
    std::fs::write(&path, bytes).expect("damage");
}

async fn report(env: &Env, pack: PackId, segment: SegmentId) -> Result<ReportOutcome, Error> {
    env.engine
        .report_corruption(&env.reader, env.cache, pack, segment)
        .await
}

#[tokio::test]
async fn a_report_about_an_intact_segment_is_not_confirmed_and_changes_nothing() {
    let env = Env::new().await;
    let (pack, built) = three_entries(&env).await;
    assert_eq!(
        report(&env, pack, built.segments[1].id).await,
        Ok(ReportOutcome::Unconfirmed)
    );
    assert_eq!(
        env.hits(&env.reader, &[1, 2, 3], false).await,
        vec![0, 1, 2]
    );
    let wrong_pack = report(&env, PackId::random(), built.segments[1].id).await;
    assert_eq!(
        wrong_pack,
        Ok(ReportOutcome::Unconfirmed),
        "a segment is only verified where it lives"
    );
    env.assert_consistent().await;
}

#[tokio::test]
async fn a_confirmed_report_deletes_only_the_entries_of_corrupt_segments() {
    let env = Env::new().await;
    let (pack, built) = three_entries(&env).await;
    damage(&env, pack, built.segments[1].offset);

    assert_eq!(
        report(&env, pack, built.segments[0].id).await,
        Ok(ReportOutcome::Unconfirmed),
        "an intact neighbour"
    );
    assert_eq!(
        report(&env, pack, built.segments[1].id).await,
        Ok(ReportOutcome::Confirmed { entries_removed: 1 })
    );
    assert_eq!(
        env.hits(&env.reader, &[1, 2, 3], false).await,
        vec![0, 2],
        "the next producer can rewrite the key"
    );
    assert_eq!(env.info().await.quota.live_bytes, 2 * 4096);
    env.assert_consistent().await;

    let again = env.artifacts(&[20], 4096);
    let (_, statuses) = env
        .publish(&env.ci, Trusted, &again, vec![entry(2, 0)])
        .await;
    assert_eq!(
        statuses,
        vec![oxabl_cache_core::domain::entry::EntryStatus::Committed]
    );
}

#[tokio::test]
async fn a_missing_object_is_corruption_and_removes_every_entry_that_named_it() {
    let env = Env::new().await;
    let (pack, built) = three_entries(&env).await;
    std::fs::remove_file(env.object_path(pack)).expect("lose the object");
    assert_eq!(
        report(&env, pack, built.segments[0].id).await,
        Ok(ReportOutcome::Confirmed { entries_removed: 3 })
    );
    assert_eq!(
        env.hits(&env.reader, &[1, 2, 3], false).await,
        Vec::<usize>::new()
    );
    env.assert_consistent().await;
}

#[tokio::test]
async fn reports_are_limited_to_ten_verifications_per_principal_per_minute() {
    let env = Env::new().await;
    let (pack, built) = three_entries(&env).await;
    for _ in 0..10 {
        assert_eq!(
            report(&env, pack, built.segments[0].id).await,
            Ok(ReportOutcome::Unconfirmed)
        );
    }
    let limited = report(&env, pack, built.segments[0].id).await;
    assert!(
        matches!(limited, Err(Error::RateLimited { retry_after_secs }) if retry_after_secs >= 1)
    );
    let peer = env
        .engine
        .report_corruption(&env.dev, env.cache, pack, built.segments[0].id)
        .await;
    assert_eq!(
        peer,
        Ok(ReportOutcome::Unconfirmed),
        "the limit is per principal"
    );
    env.clock.advance(61);
    assert_eq!(
        report(&env, pack, built.segments[0].id).await,
        Ok(ReportOutcome::Unconfirmed)
    );
}

#[tokio::test]
async fn a_report_needs_read_access_to_a_cache_of_the_callers_tenant() {
    let env = Env::new().await;
    let (pack, built) = three_entries(&env).await;
    let nobody = oxabl_cache_core::domain::principal::Caller {
        grants: vec![],
        ..env.reader.clone()
    };
    let denied = env
        .engine
        .report_corruption(&nobody, env.cache, pack, built.segments[0].id)
        .await;
    assert!(matches!(denied, Err(Error::Forbidden(_))));
    let unknown = env
        .engine
        .report_corruption(&env.reader, CacheId::random(), pack, built.segments[0].id)
        .await;
    assert_eq!(unknown, Err(Error::NotFound));
}
