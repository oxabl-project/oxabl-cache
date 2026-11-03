//! The metadata store a node runs on, and who runs garbage collection.

use crate::config::Metadata;
use oxabl_cache_core::Result as CoreResult;
use oxabl_cache_core::ports::{MetaTx, MetadataStore, TxScope};
use oxabl_cache_meta_postgres::{LeaderSession, PostgresOptions, PostgresStore};
use oxabl_cache_meta_sqlite::SqliteStore;

/// SQLite for one node, Postgres for many.
#[derive(Clone)]
pub(crate) enum AnyMeta {
    Sqlite(SqliteStore),
    Postgres(PostgresStore),
}

impl MetadataStore for AnyMeta {
    async fn transact_in<T, F>(&self, scope: TxScope, work: F) -> CoreResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> CoreResult<T> + Send + 'static,
    {
        match self {
            Self::Sqlite(store) => store.transact_in(scope, work).await,
            Self::Postgres(store) => store.transact_in(scope, work).await,
        }
    }

    async fn read<T, F>(&self, work: F) -> CoreResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> CoreResult<T> + Send + 'static,
    {
        match self {
            Self::Sqlite(store) => store.read(work).await,
            Self::Postgres(store) => store.read(work).await,
        }
    }
}

fn postgres_options(metadata: &Metadata) -> Result<Option<PostgresOptions>, String> {
    let Metadata::Postgres {
        url,
        password_file,
        ca_file,
        max_connections,
    } = metadata
    else {
        return Ok(None);
    };
    let mut options = PostgresOptions::new(url.clone());
    options.ca_file = ca_file.clone();
    options.max_connections = *max_connections;
    if let Some(path) = password_file {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read password file {}: {error}", path.display()))?;
        options.password = Some(text.trim_end_matches(['\n', '\r']).to_owned());
    }
    Ok(Some(options))
}

/// Opens the database and brings its schema up to date.
pub(crate) async fn open(metadata: &Metadata) -> Result<AnyMeta, String> {
    match (metadata, postgres_options(metadata)?) {
        (_, Some(options)) => PostgresStore::connect(&options)
            .await
            .map(AnyMeta::Postgres)
            .map_err(|error| format!("cannot open the metadata database: {error}")),
        (Metadata::Sqlite(path), None) => SqliteStore::open(path)
            .map(AnyMeta::Sqlite)
            .map_err(|error| format!("cannot open the metadata database: {error}")),
        (Metadata::Postgres { .. }, None) => unreachable!("postgres metadata has options"),
    }
}

/// Decides which node runs garbage collection.
pub(crate) enum Election {
    /// The only process of a SQLite deployment.
    Single,
    /// One of several nodes on a shared Postgres.
    Postgres(LeaderSession),
}

impl Election {
    pub(crate) fn new(metadata: &Metadata) -> Result<Self, String> {
        match postgres_options(metadata)? {
            Some(options) => LeaderSession::new(&options)
                .map(Self::Postgres)
                .map_err(|error| format!("cannot prepare the leader election: {error}")),
            None => Ok(Self::Single),
        }
    }

    /// Whether this node leads now. An error is "not the leader": a node that
    /// cannot reach the database must not collect garbage.
    pub(crate) async fn poll(&mut self) -> Result<bool, String> {
        match self {
            Self::Single => Ok(true),
            Self::Postgres(session) => session.poll().await.map_err(|error| error.to_string()),
        }
    }
}
