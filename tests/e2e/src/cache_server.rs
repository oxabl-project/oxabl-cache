//! The real `oxabl-cache-server` binary on a fresh data directory, and a small HTTP
//! client for its listeners.

use crate::postgres::Postgres;
use crate::supervise::Supervised;
use crate::wait;
use crate::workdir::TestDir;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::time::Duration;

static BUILD: Once = Once::new();

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
        PathBuf::from,
    )
}

/// Builds the server and the operator command line, once per run. Call outside the namespace.
pub fn ensure_built() {
    BUILD.call_once(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let status = Command::new(cargo)
            .arg("build")
            .arg("--manifest-path")
            .arg(manifest)
            .args([
                "-p",
                "oxabl-cache-server",
                "-p",
                "oxabl-cache-admin",
                "--locked",
            ])
            .status()
            .expect("run cargo");
        assert!(
            status.success(),
            "building the server and the admin tool failed"
        );
    });
}

fn binary() -> PathBuf {
    target_dir().join("debug/oxabl-cache-server")
}

fn admin_binary() -> PathBuf {
    target_dir().join("debug/oxabl-cache-admin")
}

/// An answer: status and body text.
pub struct Reply {
    pub status: u16,
    pub body: String,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|_| panic!("not JSON: {}", self.body))
    }
}

/// The metadata database a server runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metadata {
    /// A SQLite file in the server's directory.
    Sqlite,
    /// A fresh database on the shared Postgres of this test binary.
    Postgres,
}

/// A running server with its operator token.
pub struct CacheServer {
    _process: Supervised,
    dir: TestDir,
    pub public: String,
    pub admin: String,
    pub operator: String,
    postgres_url: Option<String>,
    http: reqwest::Client,
}

impl CacheServer {
    /// Starts a server on SQLite whose `[oidc]` section is `oidc`.
    pub async fn start(oidc: &str) -> Self {
        Self::start_on(Metadata::Sqlite, oidc).await
    }

    /// Starts a server on `metadata` whose `[oidc]` section is `oidc`.
    pub async fn start_on(metadata: Metadata, oidc: &str) -> Self {
        let dir = TestDir::new("srv").expect("test dir");
        let root = dir.path().display();
        let mut postgres_url = None;
        let metadata_url = match metadata {
            Metadata::Sqlite => format!("sqlite://{root}/meta.db"),
            Metadata::Postgres => {
                let database = tokio::task::spawn_blocking(Postgres::shared)
                    .await
                    .expect("postgres cluster")
                    .create_database()
                    .await
                    .expect("create database");
                postgres_url = Some(database.url());
                format!("{}?sslmode=disable", database.url())
            }
        };
        let config = dir.path().join("server.toml");
        fs::write(
            &config,
            format!(
                "[server]\nlisten = \"127.0.0.1:0\"\nadmin_listen = \"127.0.0.1:0\"\n\
                 metrics_listen = \"127.0.0.1:0\"\n\
                 [metadata]\nurl = \"{metadata_url}\"\n\
                 [store]\nkind = \"fs\"\nroot = \"{root}/objects\"\n\
                 [keys]\nurl_key_file = \"{root}/url.key\"\nclaim_hash_key_file = \"{root}/claim.key\"\n\
                 [oidc]\n{oidc}\n"
            ),
        )
        .expect("config");
        let binary = binary();
        let init = Command::new(&binary)
            .arg("-c")
            .arg(&config)
            .arg("init")
            .output()
            .expect("init");
        assert!(
            init.status.success(),
            "init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        let operator = String::from_utf8(init.stdout)
            .expect("utf8")
            .trim()
            .to_owned();
        let mut command = Command::new(&binary);
        command.arg("-c").arg(&config).arg("serve");
        let mut process = Supervised::spawn("server", command, dir.path()).expect("spawn server");
        let log = dir.path().join("server.log");
        let ready = wait::until(Duration::from_secs(20), || {
            process.ensure_running()?;
            let text = fs::read_to_string(&log)?;
            text.lines()
                .find(|line| line.starts_with("{\"ready\""))
                .map(str::to_owned)
                .ok_or_else(|| anyhow::anyhow!("no ready line yet"))
        })
        .expect("the server is ready");
        let ready: Value = serde_json::from_str(&ready).expect("ready line");
        let address = |key: &str| ready["ready"][key].as_str().expect("address").to_owned();
        Self {
            public: address("listen"),
            admin: address("admin_listen"),
            _process: process,
            dir,
            operator,
            postgres_url,
            http: reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("client"),
        }
    }

    async fn send(
        &self,
        addr: &str,
        token: &str,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Reply {
        let method = reqwest::Method::from_bytes(method.as_bytes()).expect("method");
        let mut request = self
            .http
            .request(method, format!("http://{addr}{path}"))
            .bearer_auth(token);
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .body(body.to_string());
        }
        let response = request.send().await.expect("request");
        let status = response.status().as_u16();
        Reply {
            status,
            body: response.text().await.expect("body"),
        }
    }

    /// A call on the public listener with `token`.
    pub async fn api(&self, token: &str, method: &str, path: &str, body: Option<&Value>) -> Reply {
        self.send(&self.public, token, method, path, body).await
    }

    /// A call on the operator listener.
    pub async fn operator(&self, method: &str, path: &str, body: Option<&Value>) -> Reply {
        self.send(&self.admin, &self.operator, method, path, body)
            .await
    }

    /// A call on the operator listener with another credential.
    pub async fn operator_as(
        &self,
        token: &str,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Reply {
        self.send(&self.admin, token, method, path, body).await
    }

    /// Runs `oxabl-cache-admin` against this server's operator listener.
    pub fn admin_cli(&self, args: &[&str]) -> std::process::Output {
        use std::os::unix::fs::PermissionsExt;
        let token_file = self.dir.path().join("operator.token");
        fs::write(&token_file, &self.operator).expect("token file");
        fs::set_permissions(&token_file, fs::Permissions::from_mode(0o600)).expect("mode");
        Command::new(admin_binary())
            .arg("--server")
            .arg(format!("http://{}", self.admin))
            .arg("--token-file")
            .arg(&token_file)
            .args(args)
            .output()
            .expect("run oxabl-cache-admin")
    }

    /// Everything the server wrote to stdout and stderr.
    pub fn log(&self) -> String {
        fs::read_to_string(self.dir.path().join("server.log")).unwrap_or_default()
    }

    /// The bytes of the metadata database: the SQLite file and its write-ahead log, or a
    /// `pg_dump` of the Postgres database followed by the decoded content of every
    /// `bytea` value in it.
    pub fn database_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        if let Some(url) = &self.postgres_url {
            let dump = Command::new("pg_dump")
                .arg(url)
                .output()
                .expect("run pg_dump");
            assert!(dump.status.success(), "pg_dump failed");
            bytes.extend_from_slice(&dump.stdout);
            bytes.extend(decoded_bytea(&dump.stdout));
            return bytes;
        }
        for name in ["meta.db", "meta.db-wal"] {
            bytes.extend(fs::read(self.dir.path().join(name)).unwrap_or_default());
        }
        bytes
    }

    /// A tenant, its administrator token and one cache.
    pub async fn tenant(&self, name: &str) -> Tenant {
        let created = self
            .operator("POST", "/admin/v1/tenants", Some(&json!({"name": name})))
            .await;
        assert_eq!(created.status, 201, "{}", created.body);
        let id = created.json()["id"].as_str().expect("id").to_owned();
        let token = self
            .operator(
                "POST",
                &format!("/admin/v1/tenants/{id}/tokens"),
                Some(&json!({"name": "admin"})),
            )
            .await;
        assert_eq!(token.status, 201, "{}", token.body);
        let admin = token.json()["token"].as_str().expect("token").to_owned();
        let owner = "07".repeat(32);
        let cache = self
            .api(
                &admin,
                "POST",
                "/v1/tenant/caches",
                Some(&json!({"display_name": "codebase", "owner_keys": [owner]})),
            )
            .await;
        assert_eq!(cache.status, 201, "{}", cache.body);
        Tenant {
            id,
            admin,
            cache: cache.json()["id"].as_str().expect("cache").to_owned(),
        }
    }
}

/// A tenant with an administrator token and a cache.
pub struct Tenant {
    pub id: String,
    pub admin: String,
    pub cache: String,
}

impl Tenant {
    pub fn audience(&self) -> String {
        format!("oxabl-cache:{}", self.id)
    }
}

/// The bytes behind every `\\x<hex>` value of a `pg_dump`, one after another.
fn decoded_bytea(dump: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::new();
    let mut at = 0;
    while at + 2 < dump.len() {
        if dump[at] == b'\\' && dump[at + 1] == b'x' {
            let digits: Vec<u8> = dump[at + 2..]
                .iter()
                .copied()
                .take_while(u8::is_ascii_hexdigit)
                .collect();
            for pair in digits.as_chunks::<2>().0 {
                let pair = std::str::from_utf8(pair).expect("hex digits");
                decoded.push(u8::from_str_radix(pair, 16).expect("hex digits"));
            }
            decoded.push(0);
            at += 2 + digits.len();
        } else {
            at += 1;
        }
    }
    decoded
}
