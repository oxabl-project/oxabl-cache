//! Several server processes on one configuration, and a direct reader of their database.

use crate::support::fixture::World;
use crate::support::frames;
use crate::support::http::request;
use crate::support::server::Client;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_oxabl-cache-server");

/// Another server process started from the configuration of `world`'s server: the
/// same database, the same bucket, its own listeners.
pub struct Node {
    child: Option<Child>,
    pub public: String,
    pub metrics: String,
}

impl Node {
    pub fn start(world: &World, name: &str) -> Self {
        let stderr =
            File::create(world.server.dir.join(format!("stderr-{name}.log"))).expect("stderr file");
        let mut child = Command::new(BIN)
            .arg("-c")
            .arg(world.server.dir.join("server.toml"))
            .arg("serve")
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn node");
        let stdout = child.stdout.take().expect("stdout");
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut lines = BufReader::new(stdout).lines();
            if let Some(Ok(line)) = lines.next() {
                let _ = sender.send(line);
            }
            for _ in lines {}
        });
        let line = receiver
            .recv_timeout(Duration::from_secs(20))
            .expect("the node reports it is ready");
        let ready: Value = serde_json::from_str(&line).expect("ready line is JSON");
        Self {
            child: Some(child),
            public: ready["ready"]["listen"]
                .as_str()
                .expect("listen")
                .to_owned(),
            metrics: ready["ready"]["metrics_listen"]
                .as_str()
                .expect("metrics")
                .to_owned(),
        }
    }

    /// A client with `client`'s token, talking to this node.
    pub fn client(&self, client: &Client) -> Client {
        Client {
            addr: self.public.clone(),
            token: client.token.clone(),
        }
    }

    /// Kills the process without letting it shut down.
    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.kill().expect("kill");
            let _ = child.wait();
        }
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Kills the node `world` started, without letting it shut down.
pub fn kill_primary(world: &World) {
    let status = Command::new("kill")
        .args(["-KILL", &world.server.pid().to_string()])
        .status()
        .expect("kill");
    assert!(status.success());
}

/// The text of a node's metrics page.
pub fn metrics(addr: &str) -> String {
    let page = request(addr, "GET", "/metrics", &[], b"");
    assert_eq!(page.status, 200);
    String::from_utf8(page.body).expect("utf8")
}

/// Whether the node reports that it runs garbage collection.
pub fn leads(metrics_addr: &str) -> bool {
    metrics(metrics_addr)
        .lines()
        .any(|line| line.trim() == "oxabl_cache_gc_leader 1")
}

/// Whether the node has ever run a garbage-collection pass.
pub fn has_collected(metrics_addr: &str) -> bool {
    metrics(metrics_addr)
        .lines()
        .any(|line| line.starts_with("oxabl_cache_gc_last_run_timestamp"))
}

/// The HTTP status of a fetch-mode lookup of `ids`.
pub fn fetch_status(world: &World, client: &Client, ids: &[[u8; 32]]) -> u16 {
    let (frame, _) = frames::lookup(ids, true, true, true, false);
    client.binary("POST", &world.path("/lookup"), &frame).status
}

/// The `[metadata] url` the server of `world` runs on.
pub fn database_url(world: &World) -> String {
    fs::read_to_string(world.server.dir.join("server.toml"))
        .expect("configuration")
        .lines()
        .find_map(|line| line.strip_prefix("url = \"postgres://"))
        .map(|rest| format!("postgres://{}", rest.trim_end_matches('"')))
        .expect("the server runs on postgres")
}

/// Runs `statement` against the database as an operator with direct access would.
pub fn sql(url: &str, statement: &str) -> Vec<tokio_postgres::Row> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls)
                .await
                .expect("connect");
            tokio::spawn(connection);
            client.query(statement, &[]).await.expect("query")
        })
}
