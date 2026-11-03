//! Versioned schema migrations applied when the database is opened.

use crate::db::DbResult;
use oxabl_cache_core::{Error, Result};
use rusqlite::Connection;

/// Migrations in order; a version is never edited once released.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../schema/0001_init.sql")),
    (2, include_str!("../schema/0002_operator_tokens.sql")),
    (3, include_str!("../schema/0003_oidc.sql")),
];

/// Applies every migration newer than the database. A database written by a
/// newer release is refused rather than opened.
pub(crate) fn migrate(conn: &Connection, now: i64) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        )",
    )
    .db()?;
    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .db()?;
    let newest = MIGRATIONS.last().map_or(0, |(version, _)| *version);
    if current > newest {
        return Err(Error::unavailable(format!(
            "database schema version {current} is newer than this release knows ({newest})"
        )));
    }
    for (version, sql) in MIGRATIONS.iter().filter(|(version, _)| *version > current) {
        conn.execute_batch("BEGIN IMMEDIATE").db()?;
        let applied = conn.execute_batch(sql).and_then(|()| {
            conn.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                (version, now),
            )
            .map(|_| ())
        });
        match applied {
            Ok(()) => conn.execute_batch("COMMIT").db()?,
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(Error::unavailable(format!(
                    "migration {version} failed: {error}"
                )));
            }
        }
    }
    Ok(())
}
