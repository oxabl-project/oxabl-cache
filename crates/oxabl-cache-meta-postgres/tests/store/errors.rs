//! What a failed statement means for the transaction and the pool.

use crate::support::fresh;
use oxabl_cache_core::Error;
use oxabl_cache_core::domain::pack::{Pack, PackState};
use oxabl_cache_core::ids::{CacheId, ClientRef, PackId, PrincipalId};
use oxabl_cache_core::ports::MetadataStore;

fn orphan_pack() -> Pack {
    Pack {
        cache: CacheId::random(),
        id: PackId::random(),
        namespace: [0; 16],
        uploader: PrincipalId::random(),
        client_ref: ClientRef::random(),
        size: 10,
        etag: None,
        live_bytes: 0,
        state: PackState::Pending,
        lease_until: 0,
        created_at: 0,
        dead_since: None,
        commit_digest: None,
        commit_statuses: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_violated_constraint_is_an_internal_error_and_the_pool_keeps_working() {
    let (store, _database) = fresh().await;
    let outcome = store.transact(|tx| tx.insert_pack(&orphan_pack())).await;
    match outcome {
        Err(Error::Internal(message)) => assert!(message.starts_with("constraint"), "{message}"),
        other => panic!("expected a constraint error, got {other:?}"),
    }
    assert_eq!(store.read(|tx| tx.live_nodes(0)).await, Ok(0));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_transaction_that_swallowed_a_failed_statement_is_not_reported_committed() {
    let (store, database) = fresh().await;
    let node = oxabl_cache_core::ids::NodeId::random();
    let outcome = store
        .transact(move |tx| {
            tx.heartbeat(&node, 5)?;
            let _ = tx.insert_pack(&orphan_pack());
            Ok(())
        })
        .await;
    assert!(
        matches!(outcome, Err(Error::Internal(_))),
        "a transaction with a failed statement cannot succeed: {outcome:?}"
    );
    let rows = database
        .connect()
        .await
        .expect("sql")
        .query_one("SELECT count(*) FROM nodes", &[])
        .await
        .expect("count");
    assert_eq!(rows.get::<_, i64>(0), 0, "nothing of it was committed");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_error_returned_by_the_work_rolls_everything_back() {
    let (store, _database) = fresh().await;
    let node = oxabl_cache_core::ids::NodeId::random();
    let outcome: Result<(), Error> = store
        .transact(move |tx| {
            tx.heartbeat(&node, 5)?;
            Err(Error::NotFound)
        })
        .await;
    assert_eq!(outcome, Err(Error::NotFound));
    assert_eq!(store.read(|tx| tx.live_nodes(0)).await, Ok(0));
}
