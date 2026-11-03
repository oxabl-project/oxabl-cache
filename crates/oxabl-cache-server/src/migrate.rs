//! `migrate`: copy the metadata of a SQLite database into an empty Postgres one.

use crate::cli::Migration;
use oxabl_cache_core::ports::migration::{MIGRATED_TABLES, TableDigest, UNMIGRATED_TABLES};
use oxabl_cache_meta_postgres::{PostgresImport, PostgresOptions};
use oxabl_cache_meta_sqlite::SqliteExport;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const BATCH_ROWS: usize = 1_000;

/// The copy holds one transaction open while it reads a whole source database.
const LONG_RUNNING: Duration = Duration::from_secs(3600);

fn options(migration: &Migration) -> Result<PostgresOptions, String> {
    let url = migration.to.as_str();
    if !(url.starts_with("postgres://") || url.starts_with("postgresql://")) {
        return Err(format!("--to `{url}` is not a postgres:// URL"));
    }
    let mut options = PostgresOptions::new(url);
    options.ca_file.clone_from(&migration.ca_file);
    options.statement_timeout = LONG_RUNNING;
    options.idle_in_transaction_timeout = LONG_RUNNING;
    if let Some(path) = &migration.password_file {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read password file {}: {error}", path.display()))?;
        options.password = Some(text.trim_end_matches(['\n', '\r']).to_owned());
    }
    Ok(options)
}

fn source(migration: &Migration) -> Result<PathBuf, String> {
    migration
        .from
        .strip_prefix("sqlite://")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| format!("--from `{}` is not a sqlite://<path> URL", migration.from))
}

fn lock(export: &Mutex<SqliteExport>) -> MutexGuard<'_, SqliteExport> {
    export
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn check_tables(tables: &[String]) -> Result<(), String> {
    let unknown: Vec<&String> = tables
        .iter()
        .filter(|table| {
            !MIGRATED_TABLES.contains(&table.as_str())
                && !UNMIGRATED_TABLES.contains(&table.as_str())
        })
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the source has tables this release does not know how to migrate: {unknown:?}"
        ))
    }
}

/// Copies every table in one Postgres transaction, then checks the row counts and a
/// content digest of every table against the source before committing. The source is
/// opened read-only.
pub(crate) async fn run(migration: &Migration) -> Result<(), String> {
    let path = source(migration)?;
    let options = options(migration)?;
    if oxabl_cache_meta_sqlite::SCHEMA_VERSION != oxabl_cache_meta_postgres::SCHEMA_VERSION {
        return Err("the two adapters write different schema versions".to_owned());
    }
    let export = Arc::new(Mutex::new(
        SqliteExport::open(&path).map_err(|error| error.to_string())?,
    ));
    let tables = lock(&export).tables().map_err(|error| error.to_string())?;
    check_tables(&tables)?;
    let import = PostgresImport::begin(&options)
        .await
        .map_err(|error| error.to_string())?;
    let mut copied = Vec::new();
    for table in MIGRATED_TABLES {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let reader = {
            let export = Arc::clone(&export);
            let table = (*table).to_owned();
            tokio::task::spawn_blocking(move || {
                lock(&export).read(&table, BATCH_ROWS, &mut |batch| {
                    sender
                        .blocking_send(batch)
                        .map_err(|_| oxabl_cache_core::Error::internal("the import stopped"))
                })
            })
        };
        let mut digest = TableDigest::default();
        let mut columns = Vec::new();
        while let Some(batch) = receiver.recv().await {
            for row in &batch.rows {
                digest.add(row);
            }
            import
                .insert(table, &batch)
                .await
                .map_err(|error| format!("copying {table}: {error}"))?;
            columns = batch.columns;
        }
        reader
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| format!("reading {table}: {error}"))?;
        if digest.rows > 0 {
            let stored = import
                .digest(table, &columns)
                .await
                .map_err(|error| format!("verifying {table}: {error}"))?;
            if stored != digest {
                return Err(format!(
                    "{table} differs after the copy: {} rows and digest {} in the source, {} rows and digest {} in the target",
                    digest.rows,
                    digest.hex(),
                    stored.rows,
                    stored.hex()
                ));
            }
        }
        copied.push((table, digest));
    }
    import.commit().await.map_err(|error| error.to_string())?;
    for (table, digest) in copied {
        println!(
            "{}",
            json!({"table": table, "rows": digest.rows, "digest": digest.hex()})
        );
    }
    Ok(())
}
