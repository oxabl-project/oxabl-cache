//! The subcommands other than `serve`.

use crate::config::Config;
use crate::{keys, wiring};
use oxabl_cache_core::ids::CacheId;
use oxabl_cache_core::ports::{MetadataStore, ObjectStore};
use serde_json::json;

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
    let app = wiring::build(config, PLACEHOLDER_URL)?;
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
    let app = wiring::build(config, PLACEHOLDER_URL)?;
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

/// Prints what the configured object store supports.
pub(crate) fn check_store(config: &Config) -> Result<(), String> {
    let app = wiring::build(config, PLACEHOLDER_URL)?;
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
