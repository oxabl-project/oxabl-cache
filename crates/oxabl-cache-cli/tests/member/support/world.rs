//! A running server with a tenant, set up through `oxabl-cache-admin` and the tenant API.

use std::fs;

use oxabl_cache_e2e::cache_server::CacheServer;
use oxabl_cache_e2e::workdir::TestDir;
use serde_json::{Value, json};

pub struct World {
    pub server: CacheServer,
    pub tenant: String,
    pub tenant_admin: String,
    pub repo: TestDir,
}

fn json_of(output: &std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "oxabl-cache-admin failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("admin JSON")
}

fn write_cache_toml(root: &std::path::Path, cache_toml: &str) {
    let dir = root.join(".oxabl");
    fs::create_dir_all(&dir).expect("directory");
    fs::write(dir.join("cache.toml"), cache_toml).expect("cache.toml");
}

impl World {
    pub async fn start() -> Self {
        Self::start_with("").await
    }

    /// A server whose `[oidc]` section is `oidc`.
    pub async fn start_with(oidc: &str) -> Self {
        oxabl_cache_e2e::cache_server::ensure_built();
        let server = CacheServer::start(oidc).await;
        let tenant = json_of(&server.admin_cli(&["--json", "tenant", "create", "acme"]))["id"]
            .as_str()
            .expect("tenant id")
            .to_owned();
        let tenant_admin = json_of(&server.admin_cli(&[
            "--json",
            "token",
            "create",
            "--tenant",
            &tenant,
            "--scopes",
            "tenant:admin",
        ]))["token"]
            .as_str()
            .expect("token")
            .to_owned();
        Self {
            server,
            tenant,
            tenant_admin,
            repo: TestDir::new("repo").expect("repo directory"),
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.server.public)
    }

    /// The tenant administrator creates a cache that accepts rosters signed by `owner_key_hex`.
    pub async fn create_cache(&self, name: &str, owner_key_hex: &str) -> String {
        let created = self
            .server
            .api(
                &self.tenant_admin,
                "POST",
                "/v1/tenant/caches",
                Some(&json!({"display_name": name, "owner_keys": [owner_key_hex]})),
            )
            .await;
        assert_eq!(created.status, 201, "{}", created.body);
        created.json()["id"].as_str().expect("cache id").to_owned()
    }

    /// The tenant administrator registers `keys` as the cache's owner keys.
    pub async fn set_owner_keys(&self, cache: &str, keys: &[String]) {
        let put = self
            .server
            .api(
                &self.tenant_admin,
                "PUT",
                &format!("/v1/tenant/caches/{cache}/owner-keys"),
                Some(&json!({"owner_keys": keys})),
            )
            .await;
        assert_eq!(put.status, 204, "{}", put.body);
    }

    /// A token with `scopes` on `cache`.
    pub async fn token(&self, name: &str, scopes: &[&str], cache: &str) -> String {
        let created = self
            .server
            .api(
                &self.tenant_admin,
                "POST",
                "/v1/tenant/tokens",
                Some(&json!({"name": name, "scopes": scopes, "caches": [cache]})),
            )
            .await;
        assert_eq!(created.status, 201, "{}", created.body);
        created.json()["token"].as_str().expect("token").to_owned()
    }

    /// Commits `.oxabl/cache.toml` text, as a repository would.
    pub fn write_repo_file(&self, cache_toml: &str) {
        write_cache_toml(self.repo.path(), cache_toml);
    }

    /// Another checkout of the repository, whose `.oxabl/cache.toml` is `cache_toml`.
    pub fn checkout(&self, cache_toml: &str) -> TestDir {
        let dir = TestDir::new("checkout").expect("checkout directory");
        write_cache_toml(dir.path(), cache_toml);
        dir
    }
}
