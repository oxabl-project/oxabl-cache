//! The usage ledger: counters, serial idempotent flushes, daily roll-up and export.

use crate::support::{Env, entry};
use oxabl_cache_core::Error;
use oxabl_cache_core::domain::namespace::NamespaceKind::Trusted;
use oxabl_cache_core::domain::usage::{Metric, UsageTotal};
use oxabl_cache_core::ports::Clock;

pub fn month_key(timestamp: i64) -> String {
    let day = timestamp.div_euclid(86_400) + 719_468;
    let era = day.div_euclid(146_097);
    let day_of_era = day - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}")
}

async fn totals(env: &Env) -> Vec<UsageTotal> {
    env.engine
        .usage_export(&month_key(env.engine.clock().now()))
        .await
        .expect("export")
}

fn value(totals: &[UsageTotal], metric: Metric) -> u64 {
    totals
        .iter()
        .filter(|t| t.metric == metric)
        .map(|t| t.value)
        .sum()
}

#[tokio::test]
async fn counters_reach_the_ledger_and_the_monthly_export() {
    let env = Env::new().await;
    let built = env.artifacts(&[1, 2], 2048);
    env.publish(&env.ci, Trusted, &built, vec![entry(1, 0), entry(2, 1)])
        .await;
    env.fetch(&env.reader, &[1, 2, 3], false).await;
    let flush = env.engine.flush_usage().await.expect("flush");
    assert!(flush.written > 0 && !flush.ignored);

    let all = totals(&env).await;
    assert_eq!(value(&all, Metric::IngressBytes), built.bytes.len() as u64);
    assert_eq!(value(&all, Metric::EntriesCommitted), 2);
    assert_eq!(value(&all, Metric::LookupIds), 3);
    assert_eq!(value(&all, Metric::Hits), 2);
    assert_eq!(value(&all, Metric::EgressBytes), 2 * 2048);
    assert_eq!(
        value(&all, Metric::Requests),
        2,
        "one commit and one lookup"
    );
    assert!(
        all.iter()
            .all(|t| t.tenant == env.tenant && t.cache == env.cache)
    );

    let own = env
        .engine
        .tenant_usage(&env.admin, &month_key(env.engine.clock().now()))
        .await
        .expect("tenant usage");
    assert_eq!(own, all);
    assert!(matches!(
        env.engine
            .tenant_usage(&env.dev, &month_key(env.engine.clock().now()))
            .await,
        Err(Error::Forbidden(_))
    ));
    assert!(matches!(
        env.engine.usage_export("2026-13").await,
        Err(Error::BadRequest(_))
    ));
    assert_eq!(
        env.engine.flush_usage().await.expect("empty flush").written,
        0,
        "nothing new, nothing written"
    );
}

#[tokio::test]
async fn a_flush_whose_acknowledgement_was_lost_is_repeated_without_counting_twice() {
    let env = Env::new().await;
    env.fetch(&env.reader, &[1], false).await;
    env.meta.lose_next_ack();
    assert!(
        matches!(env.engine.flush_usage().await, Err(Error::Unavailable(_))),
        "the write happened, the answer was lost"
    );

    env.fetch(&env.reader, &[1], false).await;
    let retry = env.engine.flush_usage().await.expect("retry");
    assert!(
        retry.ignored && retry.written == 0,
        "the same sequence number is acknowledged and ignored"
    );
    assert_eq!(
        value(&totals(&env).await, Metric::Requests),
        1,
        "the first flush counted once"
    );

    let next = env.engine.flush_usage().await.expect("next flush");
    assert!(
        !next.ignored && next.written > 0,
        "counters that arrived meanwhile go in the next flush"
    );
    assert_eq!(value(&totals(&env).await, Metric::Requests), 2);
}

#[tokio::test]
async fn a_closed_day_is_rolled_up_and_refuses_late_counters() {
    let env = Env::new().await;
    env.fetch(&env.reader, &[1], false).await;
    env.engine.flush_usage().await.expect("flush");
    env.fetch(&env.reader, &[1], false).await;
    let before = totals(&env).await;

    let until_close = 86_400 - env.engine.clock().now().rem_euclid(86_400) + 3_600;
    env.clock.advance(until_close - 1);
    assert!(
        env.engine
            .close_ledger_days()
            .await
            .expect("close")
            .is_empty(),
        "a day closes an hour after it ends"
    );
    env.clock.advance(1);
    let closed = env.engine.close_ledger_days().await.expect("close");
    assert!(
        !closed.is_empty(),
        "a day that ended more than an hour ago is closed"
    );
    assert_eq!(
        env.engine.close_ledger_days().await.expect("again"),
        Vec::<i64>::new()
    );
    assert_eq!(value(&before, Metric::Requests), 1);

    let late = env.engine.flush_usage().await.expect("late flush");
    assert!(
        late.late_dropped > 0 && late.written == 0,
        "counters for a closed day are dropped: {late:?}"
    );
    let after = env
        .engine
        .usage_export(&month_key(env.engine.clock().now() - 2 * 86_400))
        .await
        .expect("export");
    assert_eq!(
        value(&after, Metric::Requests),
        1,
        "the rolled-up day keeps its figures"
    );
}

#[tokio::test]
async fn stored_byte_snapshots_are_written_once_per_cache_per_day() {
    let env = Env::new().await;
    let built = env.artifacts(&[1], 2048);
    env.publish(&env.ci, Trusted, &built, vec![entry(1, 0)])
        .await;
    assert_eq!(
        env.engine.snapshot_stored_bytes().await.expect("snapshot"),
        1
    );
    assert_eq!(
        env.engine.snapshot_stored_bytes().await.expect("again"),
        0,
        "once a day"
    );
    assert_eq!(
        value(&totals(&env).await, Metric::StoredByteDays),
        built.bytes.len() as u64
    );
    env.clock.advance(86_400);
    assert_eq!(
        env.engine.snapshot_stored_bytes().await.expect("next day"),
        1
    );
}

#[tokio::test]
async fn the_heartbeat_counts_nodes_seen_in_the_last_thirty_seconds() {
    let env = Env::new().await;
    assert_eq!(env.engine.live_node_count().await.expect("count"), 0);
    env.engine.heartbeat().await.expect("beat");
    assert_eq!(env.engine.live_node_count().await.expect("count"), 1);
    env.clock.advance(31);
    assert_eq!(env.engine.live_node_count().await.expect("count"), 0);
    env.engine.heartbeat().await.expect("beat");
    assert_eq!(env.engine.live_node_count().await.expect("count"), 1);
}
