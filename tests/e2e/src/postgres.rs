//! A Postgres cluster in a temporary directory with a fresh database per test.

use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio_postgres::NoTls;

use crate::supervise::Supervised;
use crate::workdir::TestDir;
use crate::{cli, keeper, ports, random, wait};

const STARTUP: Duration = Duration::from_secs(60);

/// A running cluster that trusts connections from loopback.
pub struct Postgres {
    port: u16,
    _process: Supervised,
    _dir: TestDir,
}

/// A database created by [`Postgres::create_database`].
#[derive(Clone, Debug)]
pub struct Database {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub name: String,
}

impl Database {
    pub fn url(&self) -> String {
        format!(
            "postgres://{}@{}:{}/{}",
            self.user, self.host, self.port, self.name
        )
    }

    pub async fn connect(&self) -> Result<tokio_postgres::Client> {
        let (client, connection) = tokio_postgres::connect(&self.url(), NoTls).await?;
        tokio::spawn(connection);
        Ok(client)
    }
}

impl Postgres {
    /// The cluster shared by every test in this binary.
    pub fn shared() -> &'static Postgres {
        static SHARED: OnceLock<Result<Postgres, String>> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                keeper::run("postgres-shared", || wait::retry(3, Postgres::launch))
                    .map_err(|error| format!("{error:#}"))
            })
            .as_ref()
            .unwrap_or_else(|error| panic!("postgres failed to start: {error}"))
    }

    /// Runs `initdb` and starts the cluster on a free loopback port.
    ///
    /// The socket directory is the test directory, whose path can exceed the Unix socket limit,
    /// so Unix sockets are disabled and clients use TCP.
    pub fn launch() -> Result<Self> {
        let dir = TestDir::new("postgres")?;
        let port = ports::free_port();
        let data = dir.path().join("data");

        let mut initdb = Command::new("initdb");
        initdb.arg("-D").arg(&data).args([
            "-U",
            "postgres",
            "-A",
            "trust",
            "-E",
            "UTF8",
            "--locale=C",
            "--no-sync",
        ]);
        cli::run(&mut initdb)?;

        let mut server = Command::new("postgres");
        server
            .arg("-D")
            .arg(&data)
            .args(["-p", &port.to_string()])
            .args(["-c", "listen_addresses=127.0.0.1"])
            .args(["-c", "unix_socket_directories="])
            .args(["-c", "fsync=off"])
            .args(["-c", "max_connections=200"]);
        let mut process = Supervised::spawn("postgres", server, dir.path())?;
        wait::for_port(&mut process, port, STARTUP)?;

        Ok(Self {
            port,
            _process: process,
            _dir: dir,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Creates an empty database with a random name.
    pub async fn create_database(&self) -> Result<Database> {
        let admin = Database {
            host: "127.0.0.1".to_owned(),
            port: self.port,
            user: "postgres".to_owned(),
            name: "postgres".to_owned(),
        };
        let name = format!("e2e_{}", random::hex(8));
        admin
            .connect()
            .await?
            .batch_execute(&format!("CREATE DATABASE {name}"))
            .await
            .context("create database")?;
        Ok(Database { name, ..admin })
    }
}
