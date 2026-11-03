//! The configuration file as written. Every section rejects unknown keys, and no
//! secret has a field here: secrets are named by `*_file` paths only.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Raw {
    pub server: Server,
    pub tls: Option<Tls>,
    pub metadata: Metadata,
    pub store: Store,
    pub keys: Keys,
    #[serde(default)]
    pub gc: Gc,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub oidc: Oidc,
    #[serde(default)]
    pub log: Log,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Server {
    pub listen: String,
    pub admin_listen: Option<String>,
    pub metrics_listen: Option<String>,
    pub public_url: Option<String>,
    pub url_lifetime: Option<String>,
    pub shutdown_timeout: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tls {
    pub cert_file: Option<String>,
    pub key_file: Option<String>,
    #[serde(default)]
    pub terminated_upstream: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Metadata {
    pub url: String,
    pub password_file: Option<String>,
    pub ca_file: Option<String>,
    pub max_connections: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Store {
    pub kind: String,
    pub root: Option<String>,
    pub endpoint: Option<String>,
    pub region: Option<String>,
    pub bucket: Option<String>,
    pub prefix: Option<String>,
    pub path_style: Option<bool>,
    pub access_key_id_file: Option<String>,
    pub secret_access_key_file: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Keys {
    pub url_key_file: String,
    pub claim_hash_key_file: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Gc {
    pub grace: Option<String>,
    pub lease: Option<String>,
    pub interval: Option<String>,
    pub compaction_live_ratio: Option<f64>,
    pub orphan_scan: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Defaults {
    pub retention_days: Option<u32>,
    pub hard_quota: Option<String>,
    pub soft_quota_ratio: Option<f64>,
    pub rate: Option<u32>,
    pub burst: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Oidc {
    #[serde(default)]
    pub issuer_allowlist: Vec<String>,
    #[serde(default)]
    pub allowed_private_ranges: Vec<String>,
    #[serde(default)]
    pub dns_servers: Vec<String>,
    pub extra_ca_file: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Log {
    pub level: Option<String>,
    pub format: Option<String>,
}
