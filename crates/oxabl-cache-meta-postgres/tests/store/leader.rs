//! Leadership through the session-level advisory lock.

use crate::support::{fresh, options};
use oxabl_cache_meta_postgres::LeaderSession;

#[tokio::test(flavor = "multi_thread")]
async fn one_session_leads_until_its_connection_ends_and_then_another_takes_over() {
    let (_store, database) = fresh().await;
    let mut first = LeaderSession::new(&options(&database)).expect("first");
    let mut second = LeaderSession::new(&options(&database)).expect("second");

    assert!(first.poll().await.expect("poll"), "the first session leads");
    assert!(!second.poll().await.expect("poll"), "only one leads");
    assert!(first.poll().await.expect("poll"), "and keeps leading");
    assert!(!second.poll().await.expect("poll"));

    drop(first);
    let mut taken = false;
    for _ in 0..100 {
        if second.poll().await.expect("poll") {
            taken = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(taken, "the lock is free once the leader's connection ends");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_leader_whose_connection_is_cut_is_replaced() {
    let (_store, database) = fresh().await;
    let mut leader = LeaderSession::new(&options(&database)).expect("leader");
    let mut other = LeaderSession::new(&options(&database)).expect("other");
    assert!(leader.poll().await.expect("poll"));

    let admin = database.connect().await.expect("sql");
    admin
        .execute(
            "SELECT pg_terminate_backend(pid) FROM pg_locks
             WHERE locktype = 'advisory' AND granted AND pid <> pg_backend_pid()
               AND database = (SELECT oid FROM pg_database WHERE datname = current_database())",
            &[],
        )
        .await
        .expect("terminate the lock holder");

    let mut taken = false;
    for _ in 0..100 {
        if other.poll().await.expect("poll") {
            taken = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(taken, "another node takes over");
    assert!(
        !leader.poll().await.expect("poll"),
        "a leader that lost its session does not lead beside the new one"
    );
}
