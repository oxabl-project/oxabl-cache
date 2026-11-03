//! Reading a SQLite database table by table, and online backups of it.

use crate::db::DbResult;
use crate::migrate::SCHEMA_VERSION;
use oxabl_cache_core::ports::migration::{Batch, Cell};
use oxabl_cache_core::{Error, Result};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn open_read_only(path: &Path) -> Result<Connection> {
    if !path.is_file() {
        return Err(Error::bad_request(format!(
            "{} is not a SQLite database file",
            path.display()
        )));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).db()?;
    conn.busy_timeout(Duration::from_secs(30)).db()?;
    Ok(conn)
}

/// A read-only view of one consistent snapshot of a database.
pub struct SqliteExport {
    conn: Connection,
}

impl SqliteExport {
    /// Opens the database at `path` read-only and pins one snapshot. A schema other
    /// than the one this release writes is refused.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = open_read_only(path)?;
        conn.execute_batch("BEGIN").db()?;
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .db()?;
        if version != SCHEMA_VERSION {
            return Err(Error::bad_request(format!(
                "the database has schema version {version}, this release uses {SCHEMA_VERSION}: \
                 start this release on it once to bring it up to date"
            )));
        }
        Ok(Self { conn })
    }

    /// Every table of the database.
    pub fn tables(&self) -> Result<Vec<String>> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )
            .db()?;
        let names = statement.query_map([], |row| row.get(0)).db()?;
        names.collect::<rusqlite::Result<_>>().db()
    }

    /// The columns of `table`, in schema order.
    pub fn columns(&self, table: &str) -> Result<Vec<String>> {
        let mut statement = self
            .conn
            .prepare(&format!(
                "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
            ))
            .db()?;
        let names = statement.query_map([], |row| row.get(0)).db()?;
        names.collect::<rusqlite::Result<_>>().db()
    }

    /// Hands every row of `table` to `sink`, `size` rows at a time.
    pub fn read(
        &self,
        table: &str,
        size: usize,
        sink: &mut dyn FnMut(Batch) -> Result<()>,
    ) -> Result<()> {
        let columns = self.columns(table)?;
        let mut statement = self
            .conn
            .prepare(&format!("SELECT {} FROM {table}", columns.join(", ")))
            .db()?;
        let mut rows = statement.query([]).db()?;
        let mut pending = Vec::with_capacity(size);
        while let Some(row) = rows.next().db()? {
            let mut cells = Vec::with_capacity(columns.len());
            for index in 0..columns.len() {
                cells.push(match row.get_ref(index).db()? {
                    ValueRef::Null => Cell::Null,
                    ValueRef::Integer(value) => Cell::Int(value),
                    ValueRef::Blob(bytes) => Cell::Bytes(bytes.to_vec()),
                    ValueRef::Text(text) => Cell::Text(
                        String::from_utf8(text.to_vec())
                            .map_err(|_| Error::internal("stored text is not UTF-8"))?,
                    ),
                    ValueRef::Real(_) => {
                        return Err(Error::internal("a floating-point value in the metadata"));
                    }
                });
            }
            pending.push(cells);
            if pending.len() == size {
                sink(Batch {
                    columns: columns.clone(),
                    rows: std::mem::take(&mut pending),
                })?;
            }
        }
        if !pending.is_empty() {
            sink(Batch {
                columns,
                rows: pending,
            })?;
        }
        Ok(())
    }
}

fn staging_name(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

fn sync_directory(path: &Path) -> Result<()> {
    let dir = path.parent().filter(|dir| !dir.as_os_str().is_empty());
    File::open(dir.unwrap_or(Path::new(".")))
        .and_then(|dir| dir.sync_all())
        .map_err(|error| Error::unavailable(format!("cannot sync the backup directory: {error}")))
}

/// Writes a consistent copy of the database at `path` to `dest` while the server
/// that owns it keeps committing. The copy is complete and integrity-checked, or
/// `dest` does not exist; it is readable by its owner only. An existing `dest` is
/// refused.
pub fn backup(path: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        return Err(Error::bad_request(format!(
            "{} already exists",
            dest.display()
        )));
    }
    let staging = staging_name(dest);
    let _ = fs::remove_file(&staging);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&staging)
        .map_err(|error| {
            Error::unavailable(format!("cannot create {}: {error}", staging.display()))
        })?;
    let outcome = copy_and_check(path, &staging).and_then(|()| {
        File::open(&staging)
            .and_then(|file| file.sync_all())
            .map_err(|error| Error::unavailable(format!("cannot sync the backup: {error}")))?;
        fs::rename(&staging, dest)
            .map_err(|error| Error::unavailable(format!("cannot place the backup: {error}")))?;
        sync_directory(dest)
    });
    if outcome.is_err() {
        let _ = fs::remove_file(&staging);
    }
    outcome
}

fn copy_and_check(path: &Path, staging: &Path) -> Result<()> {
    let source = open_read_only(path)?;
    let target = staging
        .to_str()
        .ok_or_else(|| Error::bad_request("the backup path is not UTF-8"))?;
    source.execute("VACUUM INTO ?1", [target]).db()?;
    drop(source);
    let copy = open_read_only(staging)?;
    let verdict: String = copy
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .db()?;
    if verdict != "ok" {
        return Err(Error::internal(format!("the backup is corrupt: {verdict}")));
    }
    Ok(())
}
