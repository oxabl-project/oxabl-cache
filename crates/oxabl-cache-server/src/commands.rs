//! The subcommands other than `serve`.

use crate::config::{Config, Metadata};
use crate::{keys, wiring};
use oxabl_cache_core::ids::CacheId;
use oxabl_cache_core::ports::{MetadataStore, ObjectStore};
use serde_json::json;
use std::path::Path;

const PLACEHOLDER_URL: &str = "http://localhost";

/// Creates missing key files, opens the database (which migrates it) and prints the
/// first operator token once, on stdout.
pub(crate) async fn init(config: &Config) -> Result<(), String> {
    let mut created = vec![&config.url_key_file];
    created.extend(config.claim_hash_key_file.as_ref());
    for path in created {
        if keys::create_if_missing(path)? {
            eprintln!("created key file {}", path.display());
        }
    }
    let app = wiring::build(config, PLACEHOLDER_URL).await?;
    let exists = app
        .engine
        .operator_tokens_exist()
        .await
        .map_err(|error| error.to_string())?;
    if exists {
        return Err("an operator token already exists; init mints the first one only".to_owned());
    }
    let token = app
        .engine
        .create_operator_token("bootstrap".to_owned(), None)
        .await
        .map_err(|error| error.to_string())?;
    println!("{}", token.secret);
    eprintln!(
        "operator token printed once above; it expires at unix time {}",
        token.expires_at
    );
    Ok(())
}

/// Re-verifies the packs of one cache, or of every cache, and prints one JSON line per cache.
pub(crate) async fn scrub(config: &Config, only: Option<&str>) -> Result<(), String> {
    let app = wiring::build(config, PLACEHOLDER_URL).await?;
    let caches: Vec<CacheId> = match only {
        Some(text) => vec![
            text.parse()
                .map_err(|_| format!("`{text}` is not a cache id"))?,
        ],
        None => app
            .engine
            .metadata()
            .transact(|tx| Ok(tx.all_caches()?.into_iter().map(|cache| cache.id).collect()))
            .await
            .map_err(|error| error.to_string())?,
    };
    for cache in caches {
        let outcome = app
            .engine
            .scrub_cache(cache)
            .await
            .map_err(|error| error.to_string())?;
        println!(
            "{}",
            json!({
                "cache": cache.to_string(),
                "corrupt_segments": outcome.corrupt_segments,
                "entries_removed": outcome.entries_removed,
            })
        );
    }
    Ok(())
}

/// Writes a consistent copy of the SQLite metadata database to `to` while a server may be running.
pub(crate) fn backup(config: &Config, to: &Path) -> Result<(), String> {
    let Metadata::Sqlite(path) = &config.metadata else {
        return Err(
            "backup applies to SQLite metadata only: back up Postgres with base backups and WAL \
             archiving"
                .to_owned(),
        );
    };
    oxabl_cache_meta_sqlite::backup(path, to).map_err(|error| error.to_string())?;
    println!("{}", json!({"backup": to.display().to_string()}));
    Ok(())
}

/// Prints what the configured object store supports.
pub(crate) async fn check_store(config: &Config) -> Result<(), String> {
    let app = wiring::build(config, PLACEHOLDER_URL).await?;
    let capabilities = app.engine.object_store().capabilities();
    println!(
        "{}",
        json!({
            "kind": capabilities.kind,
            "if_none_match": capabilities.if_none_match,
            "checksum_sha256": capabilities.checksum_sha256,
        })
    );
    Ok(())
}
