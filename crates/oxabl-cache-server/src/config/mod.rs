//! The validated server configuration.

mod env;
mod metadata;
mod raw;
mod store;
mod units;

pub(crate) use metadata::Metadata;
pub(crate) use store::StoreConfig;

use oxabl_cache_core::Config as CoreConfig;
use oxabl_cache_http::log::Level;
use oxabl_cache_oidc::{Cidr, OidcSettings, Resolver};
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
    pub metadata: Metadata,
    pub store: StoreConfig,
    pub url_key_file: PathBuf,
    pub claim_hash_key_file: Option<PathBuf>,
    pub core: CoreConfig,
    pub gc: Gc,
    pub default_rate: u32,
    pub default_burst: u32,
    pub log_level: Level,
    pub oidc: OidcSettings,
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

fn dns_server(text: &str) -> Result<std::net::SocketAddr, String> {
    text.parse()
        .or_else(|_| text.parse::<std::net::IpAddr>().map(|ip| (ip, 53).into()))
        .map_err(|_| format!("oidc.dns_servers entry `{text}` is not an address"))
}

fn oidc_settings(raw: &raw::Oidc) -> Result<OidcSettings, String> {
    let resolver = if raw.dns_servers.is_empty() {
        Resolver::System
    } else {
        Resolver::Servers(
            raw.dns_servers
                .iter()
                .map(|text| dns_server(text))
                .collect::<Result<_, _>>()?,
        )
    };
    let extra_ca_pem = match raw.extra_ca_file.as_deref().filter(|path| !path.is_empty()) {
        Some(path) => Some(
            std::fs::read(path)
                .map_err(|error| format!("cannot read oidc.extra_ca_file {path}: {error}"))?,
        ),
        None => None,
    };
    Ok(OidcSettings {
        issuer_allowlist: raw.issuer_allowlist.clone(),
        allowed_private_ranges: raw
            .allowed_private_ranges
            .iter()
            .map(|text| {
                Cidr::parse(text).map_err(|error| format!("oidc.allowed_private_ranges: {error}"))
            })
            .collect::<Result<_, _>>()?,
        resolver,
        extra_ca_pem,
    })
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
        Self::read(path, true)
    }

    /// Reads and validates the file at `path` without the environment overlay, for a
    /// command that reads two configurations and so cannot tell which one a variable is for.
    pub(crate) fn load_file(path: &Path) -> Result<Self, String> {
        Self::read(path, false)
    }

    fn read(path: &Path, overlay: bool) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let mut table: toml::Table = text
            .parse()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if overlay {
            env::overlay(&mut table, std::env::vars())?;
        }
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
        let oidc = oidc_settings(&raw.oidc)?;
        let store = store::parse(raw.store)?;
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
        if let Some(batch) = raw.gc.batch {
            if batch == 0 {
                return Err("gc.batch must be positive".into());
            }
            core.gc_batch = batch;
        }
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
            metadata: metadata::parse(&raw.metadata)?,
            store,
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
            oidc,
        })
    }
}
