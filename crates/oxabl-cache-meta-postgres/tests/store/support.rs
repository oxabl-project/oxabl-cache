//! A connected store on a fresh database.

use oxabl_cache_e2e::postgres::{Database, Postgres};
use oxabl_cache_meta_postgres::{PostgresOptions, PostgresStore};

pub fn options(database: &Database) -> PostgresOptions {
    PostgresOptions::new(format!("{}?sslmode=disable", database.url()))
}

pub async fn fresh() -> (PostgresStore, Database) {
    let database = tokio::task::spawn_blocking(Postgres::shared)
        .await
        .expect("cluster")
        .create_database()
        .await
        .expect("create database");
    let store = PostgresStore::connect(&options(&database))
        .await
        .expect("connect");
    (store, database)
}
