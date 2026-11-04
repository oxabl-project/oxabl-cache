//! The metadata adapter under test: SQLite, or Postgres when `OXABL_CACHE_TEST_BACKEND=postgres`.

use oxabl_cache_core::Result;
use oxabl_cache_core::ports::{MetaTx, MetadataStore, TxScope};
use oxabl_cache_e2e::postgres::Postgres;
use oxabl_cache_meta_postgres::{PostgresOptions, PostgresStore};
use oxabl_cache_meta_sqlite::SqliteStore;
use std::path::Path;

const DATABASE_FILE: &str = "database.url";

#[derive(Clone)]
pub enum AnyMeta {
    Sqlite(SqliteStore),
    Postgres(PostgresStore),
}

impl AnyMeta {
    /// Opens the metadata that lives in `dir`; opening the same `dir` again reaches the same data.
    pub async fn open(dir: &Path) -> Self {
        if std::env::var("OXABL_CACHE_TEST_BACKEND").as_deref() != Ok("postgres") {
            return Self::Sqlite(SqliteStore::open(&dir.join("meta.db")).expect("open metadata"));
        }
        let marker = dir.join(DATABASE_FILE);
        let url = match std::fs::read_to_string(&marker) {
            Ok(url) => url,
            Err(_) => {
                let database = Postgres::shared()
                    .create_database()
                    .await
                    .expect("create database");
                let url = format!("{}?sslmode=disable", database.url());
                std::fs::write(&marker, &url).expect("record database");
                url
            }
        };
        let store = PostgresStore::connect(&PostgresOptions::new(url))
            .await
            .expect("open metadata");
        Self::Postgres(store)
    }
}

impl MetadataStore for AnyMeta {
    async fn transact_in<T, F>(&self, scope: TxScope, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static,
    {
        match self {
            Self::Sqlite(store) => store.transact_in(scope, work).await,
            Self::Postgres(store) => store.transact_in(scope, work).await,
        }
    }

    async fn read<T, F>(&self, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static,
    {
        match self {
            Self::Sqlite(store) => store.read(work).await,
            Self::Postgres(store) => store.read(work).await,
        }
    }
}
