//! Plan and fetch lookups, namespaces, URLs and the egress quota.

use crate::support::{Env, entry, lookup_id};
use oxabl_cache_core::Error;
use oxabl_cache_core::domain::namespace::NamespaceKind;
use oxabl_cache_core::domain::namespace::NamespaceKind::{Personal, Trusted};
use oxabl_cache_core::usecases::{LookupRequest, NamespaceSet};
use oxabl_cache_store_fs::UrlRequest;

fn request(env: &Env, seeds: &[u32], records: bool, urls: bool) -> LookupRequest {
    LookupRequest {
        cache: env.cache,
        namespaces: NamespaceSet {
            trusted: true,
            personal: true,
        },
        ids: seeds.iter().map(|seed| lookup_id(*seed)).collect(),
        records,
        urls,
    }
}

#[tokio::test]
async fn plan_mode_returns_presence_only_and_fetch_mode_returns_records_locations_and_urls() {
    let env = Env::new().await;
    let built = env.artifacts(&[1, 2], 2048);
    let entries = vec![entry(1, 0), entry(2, 1)];
    let records: Vec<_> = entries.iter().map(|e| e.record.clone()).collect();
    let (pack, _) = env.publish(&env.ci, Trusted, &built, entries).await;

    let plan = env
        .engine
        .lookup(&env.reader, request(&env, &[1, 2, 99], false, true))
        .await
        .expect("plan");
    assert_eq!(
        plan.hits.iter().map(|h| h.index).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(plan.hits.iter().all(|h| h.record.is_none()
        && h.location.is_none()
        && h.namespace == NamespaceKind::Trusted));
    assert!(
        plan.packs.is_empty() && plan.dictionaries.is_empty(),
        "plan mode names no packs"
    );

    let fetch = env
        .engine
        .lookup(&env.reader, request(&env, &[2, 1], true, true))
        .await
        .expect("fetch");
    assert_eq!(fetch.packs.len(), 1);
    assert_eq!(fetch.packs[0].pack, pack);
    let signed = fetch.packs[0].url.as_ref().expect("a GET URL");
    assert_eq!(signed.method, "GET");
    let exp = signed
        .url
        .split("exp=")
        .nth(1)
        .and_then(|r| r.split('&').next())
        .and_then(|v| v.parse().ok())
        .expect("exp");
    let sig = signed.url.split("sig=").nth(1).expect("sig");
    env.engine
        .object_store()
        .verify_url(
            &UrlRequest {
                method: "GET",
                cache: env.cache,
                pack,
                expires_at: exp,
                content_length: None,
                signature: sig,
            },
            oxabl_cache_core::ports::Clock::now(env.engine.clock()),
        )
        .expect("the URL verifies");
    let first = &fetch.hits[0];
    assert_eq!(first.index, 0, "request order");
    assert_eq!(first.record.as_ref(), Some(&records[1]));
    let location = first.location.expect("location");
    assert_eq!(
        (location.offset, location.length),
        (built.segments[1].offset, built.segments[1].length)
    );

    let no_urls = env
        .engine
        .lookup(&env.reader, request(&env, &[1], true, false))
        .await
        .expect("fetch");
    assert!(no_urls.packs[0].url.is_none());
}

#[tokio::test]
async fn duplicate_ids_hit_at_every_position_and_one_id_can_hit_in_both_namespaces() {
    let env = Env::new().await;
    let shared = env.artifacts(&[1], 2048);
    env.publish(&env.ci, Trusted, &shared, vec![entry(1, 0)])
        .await;
    let personal = env.artifacts(&[2], 2048);
    let mut same_key = entry(1, 0);
    same_key.record = vec![7; 256];
    env.publish(&env.dev, Personal, &personal, vec![same_key])
        .await;

    let response = env
        .engine
        .lookup(&env.dev, request(&env, &[1, 5, 1], true, false))
        .await
        .expect("lookup");
    let shape: Vec<_> = response
        .hits
        .iter()
        .map(|h| (h.index, h.namespace))
        .collect();
    assert_eq!(
        shape,
        vec![(0, Trusted), (0, Personal), (2, Trusted), (2, Personal)],
        "trusted first for each position"
    );
}

#[tokio::test]
async fn a_lookup_over_the_id_limit_is_refused() {
    let env = Env::new().await;
    let too_many: Vec<u32> = (0..10_001).collect();
    let refused = env
        .engine
        .lookup(&env.reader, request(&env, &too_many, false, false))
        .await;
    assert_eq!(refused.err(), Some(Error::TooLarge));
    let at_limit: Vec<u32> = (0..10_000).collect();
    let ok = env
        .engine
        .lookup(&env.reader, request(&env, &at_limit, false, false))
        .await
        .expect("at the limit");
    assert!(ok.hits.is_empty());
}

#[tokio::test]
async fn fetch_mode_is_refused_over_the_monthly_egress_quota_while_plan_mode_keeps_working() {
    let env = Env::with_egress(5_000).await;
    let built = env.artifacts(&[1, 2, 3], 2048);
    env.publish(
        &env.ci,
        Trusted,
        &built,
        vec![entry(1, 0), entry(2, 1), entry(3, 2)],
    )
    .await;

    env.engine
        .lookup(&env.reader, request(&env, &[1], true, false))
        .await
        .expect("first fetch");
    env.engine.flush_usage().await.expect("flush");
    env.engine
        .lookup(&env.reader, request(&env, &[1, 2, 3], true, false))
        .await
        .expect("under the quota until the next flush");
    env.engine.flush_usage().await.expect("flush");

    let refused = env
        .engine
        .lookup(&env.reader, request(&env, &[1], true, false))
        .await;
    assert_eq!(refused.err(), Some(Error::QuotaExceeded));
    let plan = env
        .engine
        .lookup(&env.reader, request(&env, &[1], false, false))
        .await
        .expect("plan mode");
    assert_eq!(plan.hits.len(), 1);
}
