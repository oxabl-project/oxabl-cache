//! The metadata backend a test binary runs on: `crate::BACKEND`.

use oxabl_cache_e2e::postgres::{Database, Postgres};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Sqlite,
    Postgres,
}

/// A fresh empty database on the cluster shared by this test binary.
pub fn new_database() -> Database {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(Postgres::shared().create_database())
        .expect("create database")
}

/// The `[metadata] url` of a node whose files live in `dir`. Postgres nodes get a
/// fresh database; nodes that share one configuration share that database.
pub fn metadata_url(dir: &Path) -> String {
    match crate::BACKEND {
        Backend::Sqlite => format!("sqlite://{}/meta.db", dir.display()),
        Backend::Postgres => format!("{}?sslmode=disable", new_database().url()),
    }
}
