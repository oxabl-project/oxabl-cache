//! MetadataStore adapter on SQLite for a single node.
//!
//! The schema uses the SQL subset shared with Postgres: integer seconds, byte and
//! text columns only, numbered `?N` placeholders (a mechanical rewrite to `$N`),
//! targeted `ON CONFLICT` clauses and no triggers. Migrations apply when the
//! database opens.

mod db;
mod export;
mod migrate;
mod store;
mod tx;

pub use export::{SqliteExport, backup};
pub use migrate::SCHEMA_VERSION;
pub use store::SqliteStore;
