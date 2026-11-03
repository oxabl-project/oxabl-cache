//! Versioned migrations under concurrent starts.

use crate::support::{fresh, options};
use oxabl_cache_core::Error;
use oxabl_cache_e2e::postgres::Postgres;
use oxabl_cache_meta_postgres::PostgresStore;

#[tokio::test(flavor = "multi_thread")]
async fn nodes_starting_together_migrate_once() {
    let database = tokio::task::spawn_blocking(Postgres::shared)
        .await
        .expect("cluster")
        .create_database()
        .await
        .expect("create database");
    let opts = options(&database);
    let starts: Vec<_> = (0..6)
        .map(|_| {
            let opts = opts.clone();
            tokio::spawn(async move { PostgresStore::connect(&opts).await.map(|_| ()) })
        })
        .collect();
    for start in starts {
        start.await.expect("task").expect("every node starts");
    }
    let client = database.connect().await.expect("sql");
    let versions: Vec<i64> = client
        .query(
            "SELECT version FROM schema_migrations ORDER BY version",
            &[],
        )
        .await
        .expect("versions")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(versions, vec![1, 2, 3, 4, 5]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_database_from_a_newer_release_is_refused() {
    let (_store, database) = fresh().await;
    database
        .connect()
        .await
        .expect("sql")
        .execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (99, 0)",
            &[],
        )
        .await
        .expect("record a newer version");
    let refused = PostgresStore::connect(&options(&database)).await;
    match refused {
        Err(Error::Unavailable(message)) => assert!(message.contains("newer"), "{message}"),
        other => panic!("expected a refusal, got {:?}", other.map(|_| ())),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_that_requires_tls_refuses_a_database_without_it() {
    let database = tokio::task::spawn_blocking(Postgres::shared)
        .await
        .expect("cluster")
        .create_database()
        .await
        .expect("create database");
    let secure = oxabl_cache_meta_postgres::PostgresOptions::new(database.url());
    assert!(
        PostgresStore::connect(&secure).await.is_err(),
        "no sslmode means TLS is required, and this server offers none"
    );
}
