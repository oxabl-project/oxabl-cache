//! MetadataStore adapter on Postgres for multiple nodes.
//!
//! The schema mirrors the SQLite adapter's: byte and text ids, integer seconds,
//! targeted `ON CONFLICT` clauses. Writers take advisory locks by scope, so
//! transactions of different caches run concurrently and those of one cache run
//! one at a time under `READ COMMITTED`; reads run on a `REPEATABLE READ`
//! snapshot and take no lock. Migrations apply at startup under an advisory lock.
//! One node at a time wins the garbage-collection leadership through a
//! session-level advisory lock.

mod error;
mod import;
mod leader;
mod lock;
mod migrate;
mod pool;
mod store;
mod tls;
mod tx;

pub use import::PostgresImport;
pub use leader::LeaderSession;
pub use migrate::SCHEMA_VERSION;
pub use store::{PostgresOptions, PostgresStore};
