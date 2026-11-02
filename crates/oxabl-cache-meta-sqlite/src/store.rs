//! The SQLite-backed `MetadataStore`: one writer connection, serialised transactions.

use crate::db::DbResult;
use crate::migrate::migrate;
use crate::tx::SqliteTx;
use oxabl_cache_core::ports::{MetaTx, MetadataStore};
use oxabl_cache_core::{Error, Result};
use rusqlite::Connection;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Metadata on a SQLite file in WAL mode with `synchronous=FULL`. Transactions
/// run one at a time on a blocking thread, each as `BEGIN IMMEDIATE`.
#[derive(Debug, Clone)]
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    /// Opens or creates the database at `path` and brings its schema up to date.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).db()?;
        conn.pragma_update(None, "journal_mode", "WAL").db()?;
        conn.pragma_update(None, "synchronous", "FULL").db()?;
        conn.pragma_update(None, "foreign_keys", "ON").db()?;
        conn.busy_timeout(std::time::Duration::from_secs(5)).db()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
            });
        migrate(&conn, now)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

fn run<T>(conn: &Connection, work: impl FnOnce(&mut dyn MetaTx) -> Result<T>) -> Result<T> {
    conn.execute_batch("BEGIN IMMEDIATE").db()?;
    let mut tx = SqliteTx::new(conn);
    match catch_unwind(AssertUnwindSafe(|| work(&mut tx))) {
        Ok(Ok(value)) => match conn.execute_batch("COMMIT") {
            Ok(()) => Ok(value),
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(Error::unavailable(format!("commit failed: {error}")))
            }
        },
        Ok(Err(error)) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
        Err(panic) => {
            let _ = conn.execute_batch("ROLLBACK");
            resume_unwind(panic)
        }
    }
}

impl MetadataStore for SqliteStore {
    async fn transact<T, F>(&self, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            run(&guard, work)
        })
        .await
        .map_err(|error| Error::internal(format!("metadata task: {error}")))?
    }
}
