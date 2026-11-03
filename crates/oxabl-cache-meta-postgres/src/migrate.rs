//! Versioned schema migrations, applied at startup under an advisory lock so that
//! nodes starting together migrate once.

use crate::error::DbResult;
use crate::lock::migration_key;
use crate::pool::Conn;
use oxabl_cache_core::{Error, Result};
use std::time::{SystemTime, UNIX_EPOCH};

/// Migrations in order; a version is never edited once released.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../schema/0001_init.sql")),
    (2, include_str!("../schema/0002_operator_tokens.sql")),
    (3, include_str!("../schema/0003_oidc.sql")),
    (4, include_str!("../schema/0004_gc_leader.sql")),
    (5, include_str!("../schema/0005_tombstones.sql")),
];

/// Applies every migration newer than the database in one transaction. A
/// database written by a newer release is refused rather than used.
pub(crate) async fn migrate(conn: &Conn) -> Result<()> {
    let client = &conn.client;
    client
        .batch_execute(&format!(
            "BEGIN; SELECT pg_advisory_xact_lock({}); \
             CREATE TABLE IF NOT EXISTS schema_migrations (
                 version BIGINT PRIMARY KEY,
                 applied_at BIGINT NOT NULL
             )",
            migration_key()
        ))
        .await
        .db()?;
    match apply(conn).await {
        Ok(()) => client.batch_execute("COMMIT").await.db(),
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            Err(error)
        }
    }
}

async fn apply(conn: &Conn) -> Result<()> {
    let client = &conn.client;
    let current: i64 = client
        .query_one(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            &[],
        )
        .await
        .db()?
        .try_get(0)
        .db()?;
    let newest = MIGRATIONS.last().map_or(0, |(version, _)| *version);
    if current > newest {
        return Err(Error::unavailable(format!(
            "database schema version {current} is newer than this release knows ({newest})"
        )));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        });
    for (version, sql) in MIGRATIONS.iter().filter(|(version, _)| *version > current) {
        client
            .batch_execute(sql)
            .await
            .map_err(|error| Error::unavailable(format!("migration {version} failed: {error}")))?;
        client
            .execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES ($1, $2)",
                &[version, &now],
            )
            .await
            .db()?;
    }
    Ok(())
}
