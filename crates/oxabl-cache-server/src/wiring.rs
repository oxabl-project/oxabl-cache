//! Building the application from a configuration.

use crate::assembler::FormatAssembler;
use crate::config::Config;
use crate::store::{self, AnyStore};
use oxabl_cache_core::Engine;
use oxabl_cache_http::{
    App, Backend, MeteredMeta, MeteredStore, Metrics, NoIdentity, TenantDefaults,
};
use oxabl_cache_meta_sqlite::SqliteStore;
use std::sync::Arc;

/// The production wiring: SQLite metadata, the configured object store, the system clock.
pub(crate) struct Prod;

impl Backend for Prod {
    type Meta = MeteredMeta<SqliteStore>;
    type Store = MeteredStore<AnyStore>;
    type Assembler = FormatAssembler;
    type Clock = oxabl_cache_core::ports::SystemClock;
    type Verifier = NoIdentity;
}

/// Opens the stores and wires the engine. `public_url` is the origin object URLs carry.
pub(crate) async fn build(config: &Config, public_url: &str) -> Result<Arc<App<Prod>>, String> {
    let metrics = Arc::new(Metrics::default());
    let meta = SqliteStore::open(&config.metadata_path)
        .map_err(|error| format!("cannot open the metadata database: {error}"))?;
    let (store, local) = store::open(config, public_url).await?;
    let engine = Engine::new(
        MeteredMeta::new(meta, Arc::clone(&metrics)),
        MeteredStore::new(store, Arc::clone(&metrics)),
        FormatAssembler,
        oxabl_cache_core::ports::SystemClock,
        config.core.clone(),
    );
    Ok(Arc::new(App::new(
        engine,
        NoIdentity,
        local,
        metrics,
        TenantDefaults {
            rate: config.default_rate,
            burst: config.default_burst,
        },
    )))
}
