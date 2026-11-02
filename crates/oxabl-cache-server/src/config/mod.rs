//! The validated server configuration.

mod env;
mod raw;
mod units;

use oxabl_cache_core::Config as CoreConfig;
use oxabl_cache_http::log::Level;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How clients reach the listeners.
#[derive(Debug, Clone)]
pub(crate) enum Transport {
    /// TLS 1.3 from certificate files.
    Tls { cert: PathBuf, key: PathBuf },
    /// Plaintext: a loopback listener, or a proxy terminates TLS upstream.
    Plaintext,
}

/// Garbage-collection timers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Gc {
    pub interval: Duration,
    pub orphan_scan: Duration,
}

/// The configuration of a node.
#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub listen: SocketAddr,
    pub admin_listen: SocketAddr,
    pub metrics_listen: SocketAddr,
    pub public_url: Option<String>,
    pub shutdown_timeout: Duration,
    pub transport: Transport,
    pub metadata_path: PathBuf,
    pub store_root: PathBuf,
    pub url_key_file: PathBuf,
    pub claim_hash_key_file: Option<PathBuf>,
    pub core: CoreConfig,
    pub gc: Gc,
    pub default_rate: u32,
    pub default_burst: u32,
    pub log_level: Level,
    pub warnings: Vec<&'static str>,
}

fn address(name: &str, text: &str) -> Result<SocketAddr, String> {
    text.parse()
        .map_err(|_| format!("{name} `{text}` is not an address such as 127.0.0.1:8443"))
}

fn seconds(duration: Duration) -> Result<i64, String> {
    i64::try_from(duration.as_secs()).map_err(|_| "duration out of range".to_owned())
}

fn permille(ratio: f64, name: &str) -> Result<u64, String> {
    if (0.0..=1.0).contains(&ratio) {
        Ok((ratio * 1000.0).round() as u64)
    } else {
        Err(format!("{name} must lie between 0 and 1"))
    }
}

fn sqlite_path(url: &str) -> Result<PathBuf, String> {
    if let Some(path) = url.strip_prefix("sqlite://") {
        return if path.is_empty() {
            Err("metadata url names no SQLite file".to_owned())
        } else {
            Ok(PathBuf::from(path))
        };
    }
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        return Err("postgres metadata is not available in this build".to_owned());
    }
    Err(format!("metadata url `{url}` is not sqlite://<path>"))
}

fn transport(
    raw: Option<&raw::Tls>,
    listeners: &[(&str, SocketAddr)],
) -> Result<Transport, String> {
    let transport = match raw {
        Some(raw::Tls {
            cert_file: Some(cert),
            key_file: Some(key),
            terminated_upstream: false,
        }) => Transport::Tls {
            cert: cert.into(),
            key: key.into(),
        },
        Some(raw::Tls {
            cert_file: None,
            key_file: None,
            terminated_upstream: true,
        }) => Transport::Plaintext,
        Some(raw::Tls {
            cert_file: None,
            key_file: None,
            terminated_upstream: false,
        })
        | None => Transport::Plaintext,
        Some(_) => {
            return Err(
                "[tls] needs cert_file and key_file together, or terminated_upstream alone"
                    .to_owned(),
            );
        }
    };
    let upstream = raw.is_some_and(|tls| tls.terminated_upstream);
    if matches!(transport, Transport::Plaintext) && !upstream {
        for (name, addr) in listeners {
            if !addr.ip().is_loopback() {
                return Err(format!(
                    "{name} {addr} is not a loopback address: configure [tls] cert_file and \
                     key_file, or set tls.terminated_upstream = true behind a TLS proxy"
                ));
            }
        }
    }
    Ok(transport)
}

impl Config {
    /// Reads, overlays with the environment and validates the file at `path`.
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let mut table: toml::Table = text
            .parse()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        env::overlay(&mut table, std::env::vars())?;
        let raw: raw::Raw = table
            .try_into()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: raw::Raw) -> Result<Self, String> {
        let listen = address("server.listen", &raw.server.listen)?;
        let admin_listen = address(
            "server.admin_listen",
            raw.server
                .admin_listen
                .as_deref()
                .unwrap_or("127.0.0.1:9091"),
        )?;
        let metrics_listen = address(
            "server.metrics_listen",
            raw.server
                .metrics_listen
                .as_deref()
                .unwrap_or("127.0.0.1:9090"),
        )?;
        let transport = transport(
            raw.tls.as_ref(),
            &[
                ("server.listen", listen),
                ("server.admin_listen", admin_listen),
            ],
        )?;
        if raw.server.public_url.is_none() && listen.ip().is_unspecified() {
            return Err(
                "server.public_url is required when server.listen is a wildcard address".to_owned(),
            );
        }
        if raw.store.kind != "fs" {
            return Err(format!(
                "store kind `{}` is not available in this build (fs only)",
                raw.store.kind
            ));
        }
        let s3_only = [
            &raw.store.endpoint,
            &raw.store.region,
            &raw.store.bucket,
            &raw.store.prefix,
            &raw.store.access_key_id_file,
            &raw.store.secret_access_key_file,
        ];
        if s3_only.iter().any(|key| key.is_some()) || raw.store.path_style.is_some() {
            return Err("store keys for s3 are set but the store kind is fs".to_owned());
        }
        if raw.metadata.password_file.is_some() {
            return Err("metadata.password_file applies to postgres only".to_owned());
        }
        let mut warnings = Vec::new();
        let oidc = &raw.oidc;
        if !(oidc.issuer_allowlist.is_empty()
            && oidc.allowed_private_ranges.is_empty()
            && oidc.dns_servers.is_empty()
            && oidc.extra_ca_file.is_none())
        {
            warnings.push("[oidc] settings are ignored: OIDC is not available in this build");
        }
        let store_root = raw.store.root.ok_or("store.root is required for kind fs")?;
        if raw
            .log
            .format
            .as_deref()
            .is_some_and(|format| format != "json")
        {
            return Err("log.format is json".to_owned());
        }
        let log_level = match raw.log.level.as_deref() {
            None => Level::Info,
            Some(text) => Level::parse(text).ok_or("log.level is debug, info, warn or error")?,
        };
        let time = |text: &Option<String>, default: &str| {
            units::duration(text.as_deref().unwrap_or(default))
        };
        let url_lifetime = time(&raw.server.url_lifetime, "15m")?;
        let grace = time(&raw.gc.grace, "24h")?;
        let lease = time(&raw.gc.lease, "1h")?;
        let interval = time(&raw.gc.interval, "10m")?;
        let orphan_scan = time(&raw.gc.orphan_scan, "7d")?;
        let shutdown_timeout = time(&raw.server.shutdown_timeout, "10s")?;
        if interval.is_zero() || url_lifetime.is_zero() || lease.is_zero() {
            return Err("gc.interval, gc.lease and server.url_lifetime must be positive".into());
        }
        let mut core = CoreConfig {
            url_lifetime_secs: seconds(url_lifetime)?,
            lease_secs: seconds(lease)?,
            grace_secs: seconds(grace)?,
            ..CoreConfig::default()
        };
        if let Some(ratio) = raw.gc.compaction_live_ratio {
            core.compaction_live_permille = permille(ratio, "gc.compaction_live_ratio")?;
        }
        if let Some(ratio) = raw.defaults.soft_quota_ratio {
            core.evict_high_permille = permille(ratio, "defaults.soft_quota_ratio")?;
            core.evict_low_permille = core.evict_high_permille.saturating_sub(100);
        }
        if let Some(days) = raw.defaults.retention_days {
            core.default_retention_days = days;
        }
        if let Some(quota) = &raw.defaults.hard_quota {
            core.default_hard_bytes = units::size(quota)?;
        }
        Ok(Self {
            listen,
            admin_listen,
            metrics_listen,
            public_url: raw.server.public_url,
            shutdown_timeout,
            transport,
            metadata_path: sqlite_path(&raw.metadata.url)?,
            store_root: store_root.into(),
            url_key_file: raw.keys.url_key_file.into(),
            claim_hash_key_file: raw.keys.claim_hash_key_file.map(PathBuf::from),
            core,
            gc: Gc {
                interval,
                orphan_scan,
            },
            default_rate: raw.defaults.rate.unwrap_or(50),
            default_burst: raw.defaults.burst.unwrap_or(200),
            log_level,
            warnings,
        })
    }
}
