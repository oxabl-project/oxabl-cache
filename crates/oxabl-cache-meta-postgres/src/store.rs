//! The Postgres-backed `MetadataStore`: pooled connections, advisory-locked writers.

use crate::error::DbResult;
use crate::lock;
use crate::migrate::migrate;
use crate::pool::{Conn, Lease, Pool};
use crate::tls::Tls;
use crate::tx::PgTx;
use oxabl_cache_core::ports::{MetaTx, MetadataStore, TxScope};
use oxabl_cache_core::{Error, Result};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;
use tokio_postgres::config::SslMode;

const STATEMENT_TIMEOUT_MS: u64 = 120_000;
const IDLE_IN_TRANSACTION_MS: u64 = 60_000;
const KEEPALIVE: Duration = Duration::from_secs(30);

/// How to reach the database.
#[derive(Debug, Clone)]
pub struct PostgresOptions {
    /// A `postgres://` URL. TLS is required unless the URL says `sslmode=disable`.
    pub url: String,
    /// Replaces any password in the URL.
    pub password: Option<String>,
    /// Trusts only the certificates in this PEM file instead of the public roots.
    pub ca_file: Option<PathBuf>,
    /// The most connections open at once.
    pub max_connections: usize,
    /// How long opening one connection may take.
    pub connect_timeout: Duration,
}

impl PostgresOptions {
    /// Options for `url` with a pool of 16 connections.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            password: None,
            ca_file: None,
            max_connections: 16,
            connect_timeout: Duration::from_secs(10),
        }
    }
}

/// Metadata on Postgres. Writers take advisory locks by scope, so transactions
/// of different caches run concurrently and those of one cache run one at a time;
/// reads run on a snapshot and take no lock.
#[derive(Clone)]
pub struct PostgresStore {
    pool: Arc<Pool>,
}

pub(crate) fn connection_config(
    options: &PostgresOptions,
) -> Result<(tokio_postgres::Config, Option<Tls>)> {
    let mut config: tokio_postgres::Config = options
        .url
        .parse()
        .map_err(|error| Error::unavailable(format!("metadata url: {error}")))?;
    if let Some(password) = &options.password {
        config.password(password);
    }
    let tls = if config.get_ssl_mode() == SslMode::Disable {
        None
    } else {
        config.ssl_mode(SslMode::Require);
        Some(Tls::new(options.ca_file.as_deref())?)
    };
    config
        .connect_timeout(options.connect_timeout)
        .keepalives(true)
        .keepalives_idle(KEEPALIVE)
        .application_name("oxabl-cache-server")
        .options(format!(
            "-c statement_timeout={STATEMENT_TIMEOUT_MS} \
             -c idle_in_transaction_session_timeout={IDLE_IN_TRANSACTION_MS}"
        ));
    Ok((config, tls))
}

impl PostgresStore {
    /// Connects, brings the schema up to date and returns the store.
    pub async fn connect(options: &PostgresOptions) -> Result<Self> {
        if options.max_connections == 0 {
            return Err(Error::bad_request("the pool needs at least one connection"));
        }
        let (config, tls) = connection_config(options)?;
        let first = Conn::connect(&config, tls.as_ref()).await?;
        migrate(&first).await?;
        Ok(Self {
            pool: Pool::new(config, tls, options.max_connections),
        })
    }

    async fn run<T, F>(&self, scope: Option<TxScope>, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static,
    {
        let mut lease = self.pool.lease().await?;
        let handle = Handle::current();
        tokio::task::spawn_blocking(move || execute(&mut lease, &handle, scope, work))
            .await
            .map_err(|error| Error::internal(format!("metadata task: {error}")))?
    }
}

fn begin(scope: Option<TxScope>) -> String {
    match scope {
        Some(scope) => format!(
            "BEGIN ISOLATION LEVEL READ COMMITTED; {}",
            lock::acquire(scope)
        ),
        None => "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY".to_owned(),
    }
}

fn execute<T>(
    lease: &mut Lease,
    handle: &Handle,
    scope: Option<TxScope>,
    work: impl FnOnce(&mut dyn MetaTx) -> Result<T>,
) -> Result<T> {
    if let Err(error) = handle
        .block_on(lease.client.batch_execute(&begin(scope)))
        .db()
    {
        lease.reusable = false;
        return Err(error);
    }
    let (outcome, failed) = {
        let mut tx = PgTx::new(lease, handle);
        let outcome = catch_unwind(AssertUnwindSafe(|| work(&mut tx)));
        (outcome, tx.failed())
    };
    let end = |lease: &mut Lease, sql: &str| {
        let ended = handle.block_on(lease.client.batch_execute(sql));
        if ended.is_err() {
            lease.reusable = false;
        }
        ended.db()
    };
    match outcome {
        Ok(Ok(_)) if failed => {
            let _ = end(lease, "ROLLBACK");
            Err(Error::internal(
                "a statement failed inside a transaction that reported success",
            ))
        }
        Ok(Ok(value)) => match end(lease, "COMMIT") {
            Ok(()) => Ok(value),
            Err(error) => Err(Error::unavailable(format!("commit failed: {error}"))),
        },
        Ok(Err(error)) => {
            let _ = end(lease, "ROLLBACK");
            Err(error)
        }
        Err(panic) => {
            let _ = end(lease, "ROLLBACK");
            resume_unwind(panic)
        }
    }
}

impl MetadataStore for PostgresStore {
    async fn transact_in<T, F>(&self, scope: TxScope, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static,
    {
        self.run(Some(scope), work).await
    }

    async fn read<T, F>(&self, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn MetaTx) -> Result<T> + Send + 'static,
    {
        self.run(None, work).await
    }
}
