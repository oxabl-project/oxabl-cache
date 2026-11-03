//! Issuer key sets: fetched through the guard, cached for at most an hour.

use crate::fetch::Fetcher;
use crate::jwk::{Jwk, parse_jwks};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const MAX_AGE: Duration = Duration::from_secs(60 * 60);
const REFETCH_COOLDOWN: Duration = Duration::from_secs(1);

struct Cached {
    fetched: Instant,
    keys: Arc<Vec<Jwk>>,
}

/// Why keys could not be provided.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct KeysUnavailable;

/// Fetches the key sets of issuers and keeps each for at most an hour.
pub(crate) struct KeySets {
    fetcher: Arc<Fetcher>,
    entries: std::sync::Mutex<HashMap<String, Arc<Mutex<Option<Cached>>>>>,
}

/// Fetches discovery and the key set of an issuer, uncached.
pub(crate) async fn fetch_keys(
    fetcher: &Fetcher,
    issuer: &str,
) -> Result<Vec<Jwk>, KeysUnavailable> {
    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let discovery = fetcher
        .get(&discovery_url)
        .await
        .map_err(|_| KeysUnavailable)?;
    let document: Value = serde_json::from_slice(&discovery).map_err(|_| KeysUnavailable)?;
    if document.get("issuer").and_then(Value::as_str) != Some(issuer) {
        return Err(KeysUnavailable);
    }
    let jwks_uri = document
        .get("jwks_uri")
        .and_then(Value::as_str)
        .ok_or(KeysUnavailable)?;
    let jwks = fetcher.get(jwks_uri).await.map_err(|_| KeysUnavailable)?;
    match parse_jwks(&jwks) {
        Some(keys) if !keys.is_empty() => Ok(keys),
        _ => Err(KeysUnavailable),
    }
}

impl KeySets {
    pub fn new(fetcher: Arc<Fetcher>) -> Self {
        Self {
            fetcher,
            entries: std::sync::Mutex::default(),
        }
    }

    fn slot(&self, issuer: &str) -> Arc<Mutex<Option<Cached>>> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(entries.entry(issuer.to_owned()).or_default())
    }

    /// The issuer's keys. A set older than an hour is refetched. A `kid` the cached set
    /// lacks refetches it once, unless it was fetched a moment ago. When the hour is up
    /// and the refetch fails there are no keys.
    pub async fn keys(
        &self,
        issuer: &str,
        kid: Option<&str>,
    ) -> Result<Arc<Vec<Jwk>>, KeysUnavailable> {
        let slot = self.slot(issuer);
        let mut cached = slot.lock().await;
        let usable = cached
            .as_ref()
            .is_some_and(|c| c.fetched.elapsed() < MAX_AGE);
        if usable {
            let held = cached.as_ref().expect("checked above");
            let known = kid.is_none_or(|kid| held.keys.iter().any(|key| key.kid() == Some(kid)));
            if known || held.fetched.elapsed() < REFETCH_COOLDOWN {
                return Ok(Arc::clone(&held.keys));
            }
        }
        match fetch_keys(&self.fetcher, issuer).await {
            Ok(keys) => {
                let keys = Arc::new(keys);
                *cached = Some(Cached {
                    fetched: Instant::now(),
                    keys: Arc::clone(&keys),
                });
                Ok(keys)
            }
            Err(failure) => {
                if !usable {
                    *cached = None;
                }
                match cached.as_ref() {
                    Some(held) if usable => Ok(Arc::clone(&held.keys)),
                    _ => Err(failure),
                }
            }
        }
    }
}
