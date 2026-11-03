//! `migrate`: move a SQLite deployment onto Postgres, packs included.

use crate::cli::Migration;
use crate::config::{Config, Metadata};
use crate::migrate_packs::{self, Moved};
use crate::{meta, store};
use oxabl_cache_core::ports::migration::{MIGRATED_TABLES, TableDigest, UNMIGRATED_TABLES};
use oxabl_cache_meta_postgres::{PostgresImport, PostgresOptions};
use oxabl_cache_meta_sqlite::SqliteExport;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

const BATCH_ROWS: usize = 1_000;
const PLACEHOLDER_URL: &str = "http://localhost";

/// The copy holds one transaction open while it reads a whole source database.
const LONG_RUNNING: Duration = Duration::from_secs(3600);

/// Source and target configurations, checked for what `migrate` moves between.
struct Deployments {
    source: Config,
    sqlite: PathBuf,
    target: Config,
    options: PostgresOptions,
}

fn load(migration: &Migration) -> Result<Deployments, String> {
    let source = Config::load_file(&migration.from)?;
    let target = Config::load_file(&migration.to)?;
    let Metadata::Sqlite(sqlite) = &source.metadata else {
        return Err(format!(
            "--from {} does not use SQLite metadata",
            migration.from.display()
        ));
    };
    let sqlite = sqlite.clone();
    let Some(mut options) = meta::postgres_options(&target.metadata)? else {
        return Err(format!(
            "--to {} does not use Postgres metadata",
            migration.to.display()
        ));
    };
    options.statement_timeout = LONG_RUNNING;
    options.idle_in_transaction_timeout = LONG_RUNNING;
    Ok(Deployments {
        source,
        sqlite,
        target,
        options,
    })
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

/// Verifies every committed pack in the source store and writes it to the target store, then
/// copies every table in one Postgres transaction. Each table's row count and content digest
/// are checked against the source before the commit, and the commit is the last step: a pack
/// that fails verification leaves the target without a metadata row. The source is opened
/// read-only.
pub(crate) async fn run(migration: &Migration) -> Result<(), String> {
    let Deployments {
        source,
        sqlite,
        target,
        options,
    } = load(migration)?;
    if oxabl_cache_meta_sqlite::SCHEMA_VERSION != oxabl_cache_meta_postgres::SCHEMA_VERSION {
        return Err("the two adapters write different schema versions".to_owned());
    }
    let export = Arc::new(Mutex::new(
        SqliteExport::open(&sqlite).map_err(|error| error.to_string())?,
    ));
    let tables = lock(&export).tables().map_err(|error| error.to_string())?;
    check_tables(&tables)?;
    PostgresImport::begin(&options)
        .await
        .map_err(|error| error.to_string())?;

    let packs = migrate_packs::live_packs(&lock(&export))?;
    let source_store = store::open_for_reading(&source)?;
    let in_place = store::same_store(&source.store, &target.store);
    let target_store = if in_place {
        None
    } else {
        Some(store::open(&target, PLACEHOLDER_URL).await?.0)
    };
    let moved = migrate_packs::move_packs(&packs, &source_store, target_store.as_ref()).await?;

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
    let Moved { packs, bytes } = moved;
    println!(
        "{}",
        json!({"packs": packs, "bytes": bytes, "copied": !in_place})
    );
    Ok(())
}
