//! What the local-disk object route asks the core before it moves bytes.

use crate::support::{Env, entry};
use oxabl_cache_core::domain::namespace::NamespaceKind::Trusted;
use oxabl_cache_core::domain::usage::Metric;
use oxabl_cache_core::ids::PackId;
use oxabl_cache_core::usecases::UploadOutcome;
use oxabl_cache_core::{Conflict, Error};

#[tokio::test]
async fn a_put_is_accepted_only_for_a_pending_pack_of_the_reserved_size() {
    let env = Env::new().await;
    let built = env.artifacts(&[1], 2048);
    let size = built.bytes.len() as u64;
    let UploadOutcome::Reserved { pack, url } = env
        .reserve(&env.ci, Trusted, &built)
        .await
        .expect("reserve")
    else {
        panic!("expected a reservation");
    };
    assert_eq!(
        env.engine.pack_accepts_put(env.cache, pack, size).await,
        Ok(())
    );
    assert!(matches!(
        env.engine.pack_accepts_put(env.cache, pack, size + 1).await,
        Err(Error::BadRequest(_))
    ));
    assert_eq!(
        env.engine
            .pack_accepts_put(env.cache, PackId::random(), size)
            .await,
        Err(Error::NotFound)
    );

    env.put_through_url(&url, pack, &built.bytes).expect("put");
    assert_eq!(
        env.engine.pack_accepts_put(env.cache, pack, size).await,
        Ok(()),
        "a retried PUT is fine while pending"
    );
    env.commit(&env.ci, pack, Trusted, &built, vec![entry(1, 0)])
        .await
        .expect("commit");
    assert_eq!(
        env.engine.pack_accepts_put(env.cache, pack, size).await,
        Err(Error::Conflict(Conflict::PackClosed)),
        "no PUT once the pack is committed"
    );
}

#[tokio::test]
async fn a_get_is_served_only_for_live_packs_and_served_bytes_are_counted_exactly() {
    let env = Env::new().await;
    let built = env.artifacts(&[1], 2048);
    let UploadOutcome::Reserved { pack, url } = env
        .reserve(&env.ci, Trusted, &built)
        .await
        .expect("reserve")
    else {
        panic!("expected a reservation");
    };
    assert_eq!(
        env.engine.pack_serves_get(env.cache, pack).await,
        Err(Error::NotFound),
        "pending packs are not served"
    );
    env.put_through_url(&url, pack, &built.bytes).expect("put");
    env.commit(&env.ci, pack, Trusted, &built, vec![entry(1, 0)])
        .await
        .expect("commit");
    assert_eq!(env.engine.pack_serves_get(env.cache, pack).await, Ok(()));

    env.engine
        .count_served_bytes(env.cache, 1234)
        .await
        .expect("count");
    env.engine.flush_usage().await.expect("flush");
    let totals = env
        .engine
        .usage_export(&crate::usage_ledger::month_key(
            oxabl_cache_core::ports::Clock::now(env.engine.clock()),
        ))
        .await
        .expect("export");
    let egress: u64 = totals
        .iter()
        .filter(|t| t.metric == Metric::EgressBytes)
        .map(|t| t.value)
        .sum();
    assert_eq!(egress, 1234);
}
