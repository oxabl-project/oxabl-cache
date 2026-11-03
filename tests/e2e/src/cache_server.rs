//! The real `oxabl-cache-server` binary on a fresh data directory, and a small HTTP
//! client for its listeners.

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

/// Builds the server binary, once per run. Call outside the namespace.
pub fn ensure_built() {
    BUILD.call_once(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let status = Command::new(cargo)
            .args(["build", "-p", "oxabl-cache-server", "--locked"])
            .status()
            .expect("run cargo");
        assert!(status.success(), "building oxabl-cache-server failed");
    });
}

fn binary() -> PathBuf {
    target_dir().join("debug/oxabl-cache-server")
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

/// A running server with its operator token.
pub struct CacheServer {
    _process: Supervised,
    dir: TestDir,
    pub public: String,
    pub admin: String,
    pub operator: String,
    http: reqwest::Client,
}

impl CacheServer {
    /// Starts a server whose `[oidc]` section is `oidc`.
    pub async fn start(oidc: &str) -> Self {
        let dir = TestDir::new("srv").expect("test dir");
        let root = dir.path().display();
        let config = dir.path().join("server.toml");
        fs::write(
            &config,
            format!(
                "[server]\nlisten = \"127.0.0.1:0\"\nadmin_listen = \"127.0.0.1:0\"\n\
                 metrics_listen = \"127.0.0.1:0\"\n\
                 [metadata]\nurl = \"sqlite://{root}/meta.db\"\n\
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

    /// Everything the server wrote to stdout and stderr.
    pub fn log(&self) -> String {
        fs::read_to_string(self.dir.path().join("server.log")).unwrap_or_default()
    }

    /// The bytes of the metadata database and its write-ahead log.
    pub fn database_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
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
