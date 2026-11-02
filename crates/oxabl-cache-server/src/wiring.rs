//! Building the application from a configuration.

use crate::assembler::FormatAssembler;
use crate::config::Config;
use crate::keys;
use oxabl_cache_core::Engine;
use oxabl_cache_http::{
    App, Backend, MeteredMeta, MeteredStore, Metrics, NoIdentity, TenantDefaults,
};
use oxabl_cache_meta_sqlite::SqliteStore;
use oxabl_cache_store_fs::FsStore;
use std::sync::Arc;

/// The production wiring: SQLite metadata, a local-disk store, the system clock.
pub(crate) struct Prod;

impl Backend for Prod {
    type Meta = MeteredMeta<SqliteStore>;
    type Store = MeteredStore<FsStore>;
    type Assembler = FormatAssembler;
    type Clock = oxabl_cache_core::ports::SystemClock;
    type Verifier = NoIdentity;
}

/// Opens the stores and wires the engine. `public_url` is the origin object URLs carry.
pub(crate) fn build(config: &Config, public_url: &str) -> Result<Arc<App<Prod>>, String> {
    let url_keys = keys::read_url_keys(&config.url_key_file)?;
    let metrics = Arc::new(Metrics::default());
    let meta = SqliteStore::open(&config.metadata_path)
        .map_err(|error| format!("cannot open the metadata database: {error}"))?;
    let store = FsStore::open(config.store_root.clone(), public_url, url_keys)
        .map_err(|error| format!("cannot open the object store: {error}"))?;
    let engine = Engine::new(
        MeteredMeta::new(meta, Arc::clone(&metrics)),
        MeteredStore::new(store.clone(), Arc::clone(&metrics)),
        FormatAssembler,
        oxabl_cache_core::ports::SystemClock,
        config.core.clone(),
    );
    Ok(Arc::new(App::new(
        engine,
        NoIdentity,
        Some(store),
        metrics,
        TenantDefaults {
            rate: config.default_rate,
            burst: config.default_burst,
        },
    )))
}
