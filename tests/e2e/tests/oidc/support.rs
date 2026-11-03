use oxabl_cache_e2e::cache_server::{CacheServer, Reply, Tenant};
use oxabl_cache_e2e::fake_dns::FakeDns;
use oxabl_cache_e2e::idp::ca::TestCa;
use oxabl_cache_e2e::idp::keys::{Alg, SigningKey};
use oxabl_cache_e2e::idp::server::{Decoy, Idp};
use oxabl_cache_e2e::workdir::TestDir;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;

pub const PUBLIC: &str = "10.99.0.10";
pub const PRIVATE: &str = "10.0.0.5";
pub const ADDRESSES: [&str; 2] = [PUBLIC, PRIVATE];

pub const REPOSITORY: &str = "acme/widgets";
pub const REPOSITORY_ID: &str = "918273645";

pub fn key(alg: Alg, kid: &str) -> Arc<SigningKey> {
    Arc::new(SigningKey::generate(alg, kid))
}

/// A tenant on a running server, with a fake IdP at `PUBLIC` and a decoy at `PRIVATE`.
pub struct Env {
    pub dns: FakeDns,
    pub idp: Idp,
    pub decoy: Decoy,
    pub server: CacheServer,
    pub tenant: Tenant,
    _ca_dir: TestDir,
}

impl Env {
    pub async fn start() -> Self {
        Self::with("").await
    }

    /// `extra` is added to the `[oidc]` section.
    pub async fn with(extra: &str) -> Self {
        let ca = TestCa::new();
        let ca_dir = TestDir::new("ca").expect("ca dir");
        let ca_file = ca_dir.path().join("ca.pem");
        std::fs::write(&ca_file, ca.pem()).expect("ca file");
        let dns = FakeDns::start("127.0.0.1:0".parse().expect("addr")).await;
        for name in ["idp.test", "rebind.test"] {
            dns.set(name, PUBLIC);
        }
        dns.set("private.test", PRIVATE);
        let at = |host: &str| -> SocketAddr { format!("{host}:443").parse().expect("addr") };
        let idp = Idp::start(at(PUBLIC), &ca, &["idp.test", "rebind.test"]).await;
        let decoy = Decoy::start(at(PRIVATE)).await;
        let server = CacheServer::start(&format!(
            "allowed_private_ranges = [\"{PUBLIC}/32\"]\ndns_servers = [\"{}\"]\nextra_ca_file = \"{}\"\n{extra}",
            dns.addr,
            ca_file.display()
        ))
        .await;
        let tenant = server.tenant("acme").await;
        Self {
            dns,
            idp,
            decoy,
            server,
            tenant,
            _ca_dir: ca_dir,
        }
    }

    pub async fn admin(&self, method: &str, path: &str, body: Option<&Value>) -> Reply {
        self.server
            .api(&self.tenant.admin, method, path, body)
            .await
    }

    pub async fn register(&self, issuer: &str, shape: &str, id_claims: &[&str]) -> Reply {
        self.admin(
            "POST",
            "/v1/tenant/oidc/issuers",
            Some(&json!({"issuer": issuer, "shape": shape, "id_claims": id_claims})),
        )
        .await
    }

    pub async fn policy(&self, name: &str, issuer: &str, claims: Value, scopes: &[&str]) -> Reply {
        self.admin(
            "POST",
            "/v1/tenant/oidc/policies",
            Some(&json!({
                "name": name,
                "issuer": issuer,
                "claims": claims,
                "grants": [{"cache": self.tenant.cache, "scopes": scopes}],
            })),
        )
        .await
    }

    pub async fn exchange_for(&self, tenant: &str, id_token: &str) -> Reply {
        self.server
            .api(
                "",
                "POST",
                "/v1/auth/oidc",
                Some(&json!({"tenant": tenant, "id_token": id_token})),
            )
            .await
    }

    pub async fn exchange(&self, id_token: &str) -> Reply {
        self.exchange_for(&self.tenant.id, id_token).await
    }

    /// Registers the GitHub-shaped issuer at `prefix` with `key` and a writer policy.
    pub async fn github_writer(&self, prefix: &str, key: Arc<SigningKey>) -> String {
        let issuer = format!("https://idp.test{prefix}");
        self.idp.publish(prefix, vec![key]);
        let registered = self.register(&issuer, "github", &[]).await;
        assert_eq!(registered.status, 201, "{}", registered.body);
        let policy = self
            .policy(
                "writer",
                &issuer,
                json!({
                    "repository_id": REPOSITORY_ID,
                    "ref": "refs/heads/main",
                    "environment": "cache-writer",
                }),
                &["read", "write:trusted"],
            )
            .await;
        assert_eq!(policy.status, 201, "{}", policy.body);
        issuer
    }
}

pub fn error_code(reply: &Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

pub fn token_of(reply: &Reply) -> String {
    reply.json()["token"].as_str().expect("token").to_owned()
}
