//! A bounded pool of database connections, replaced when one breaks.

use crate::error::DbResult;
use crate::tls::Tls;
use oxabl_cache_core::{Error, Result};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_postgres::{Client, Config, NoTls, Statement};

/// One connection and the statements it has prepared.
pub(crate) struct Conn {
    pub(crate) client: Client,
    statements: Mutex<HashMap<String, Statement>>,
}

impl Conn {
    pub(crate) async fn connect(config: &Config, tls: Option<&Tls>) -> Result<Self> {
        let client = match tls {
            Some(tls) => {
                let (client, connection) = config.connect(tls.clone()).await.db()?;
                tokio::spawn(connection);
                client
            }
            None => {
                let (client, connection) = config.connect(NoTls).await.db()?;
                tokio::spawn(connection);
                client
            }
        };
        Ok(Self {
            client,
            statements: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn cached(&self, sql: &str) -> Option<Statement> {
        let statements = self.statements.lock().unwrap_or_else(|p| p.into_inner());
        statements.get(sql).cloned()
    }

    pub(crate) fn remember(&self, sql: &str, statement: &Statement) {
        let mut statements = self.statements.lock().unwrap_or_else(|p| p.into_inner());
        statements.insert(sql.to_owned(), statement.clone());
    }
}

pub(crate) struct Pool {
    config: Config,
    tls: Option<Tls>,
    idle: Mutex<Vec<Conn>>,
    permits: Arc<Semaphore>,
}

impl Pool {
    pub(crate) fn new(config: Config, tls: Option<Tls>, size: usize) -> Arc<Self> {
        Arc::new(Self {
            config,
            tls,
            idle: Mutex::new(Vec::new()),
            permits: Arc::new(Semaphore::new(size)),
        })
    }

    /// A connection that is not in use, opening a new one when none is idle.
    pub(crate) async fn lease(self: &Arc<Self>) -> Result<Lease> {
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| Error::internal("connection pool closed"))?;
        let idle = loop {
            let candidate = self.idle.lock().unwrap_or_else(|p| p.into_inner()).pop();
            match candidate {
                Some(conn) if conn.client.is_closed() => {}
                other => break other,
            }
        };
        let conn = match idle {
            Some(conn) => conn,
            None => Conn::connect(&self.config, self.tls.as_ref()).await?,
        };
        Ok(Lease {
            conn: Some(conn),
            reusable: true,
            pool: Arc::clone(self),
            _permit: permit,
        })
    }
}

/// A connection on loan. It returns to the pool on drop unless it was marked
/// unusable: a connection with an unknown transaction state is closed instead.
pub(crate) struct Lease {
    conn: Option<Conn>,
    pub(crate) reusable: bool,
    pool: Arc<Pool>,
    _permit: OwnedSemaphorePermit,
}

impl Deref for Lease {
    type Target = Conn;

    fn deref(&self) -> &Conn {
        self.conn.as_ref().expect("a lease holds its connection")
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            if self.reusable && !conn.client.is_closed() {
                self.pool
                    .idle
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(conn);
            }
        }
    }
}
