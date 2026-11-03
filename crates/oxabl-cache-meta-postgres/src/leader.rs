//! Garbage-collection leadership through a session-level advisory lock.

use crate::error::DbResult;
use crate::lock::leader_key;
use crate::pool::Conn;
use crate::store::PostgresOptions;
use crate::store::connection_config;
use crate::tls::Tls;
use oxabl_cache_core::Result;
use tokio_postgres::Config;

/// A dedicated connection that holds the leadership lock for as long as it lives.
/// The lock is released by the database when the connection ends, whether the
/// process exits cleanly or dies.
pub struct LeaderSession {
    config: Config,
    tls: Option<Tls>,
    conn: Option<Conn>,
    held: bool,
}

impl LeaderSession {
    /// Prepares a session for the database of `store`'s options; it connects on demand.
    pub fn new(options: &PostgresOptions) -> Result<Self> {
        let (config, tls) = connection_config(options)?;
        Ok(Self {
            config,
            tls,
            conn: None,
            held: false,
        })
    }

    /// Whether this session holds the leadership lock. Call it periodically: it
    /// takes the lock when it is free and notices a lost connection, which gives
    /// the leadership up.
    pub async fn poll(&mut self) -> Result<bool> {
        if self
            .conn
            .as_ref()
            .is_some_and(|conn| conn.client.is_closed())
        {
            self.conn = None;
            self.held = false;
        }
        if self.held {
            if self.alive().await {
                return Ok(true);
            }
            self.conn = None;
            self.held = false;
        }
        if self.conn.is_none() {
            self.conn = Some(Conn::connect(&self.config, self.tls.as_ref()).await?);
        }
        let Some(conn) = &self.conn else {
            return Ok(false);
        };
        let acquired: bool = conn
            .client
            .query_one("SELECT pg_try_advisory_lock($1)", &[&leader_key()])
            .await
            .db()?
            .try_get(0)
            .db()?;
        self.held = acquired;
        Ok(acquired)
    }

    async fn alive(&self) -> bool {
        match &self.conn {
            Some(conn) => conn.client.simple_query("SELECT 1").await.is_ok(),
            None => false,
        }
    }
}
