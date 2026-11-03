use std::fs;
use std::io::Read;
use std::path::Path;

use oxabl_cache_format::{CacheId, PublicKey, TenantId};
use serde::Deserialize;
use url::Url;

use super::error::ConfigError;
use super::owner_key::parse_owner_key;

const KEYS: [&str; 5] = [
    "server",
    "tenant",
    "cache",
    "owner_keys",
    "min_roster_version",
];
const MAX_BYTES: u64 = 64 * 1024;

/// The contents of `.oxabl/cache.toml`: the cache identity and the root of trust.
pub(crate) struct RepoFile {
    pub server: Url,
    pub tenant: TenantId,
    pub cache: CacheId,
    pub owner_keys: Vec<PublicKey>,
    pub min_roster_version: u64,
}

#[derive(Deserialize)]
struct Raw {
    server: String,
    tenant: String,
    cache: String,
    owner_keys: Vec<String>,
    min_roster_version: u64,
}

fn io(error: std::io::Error) -> ConfigError {
    ConfigError::Io(error.kind())
}

fn locate(root: &Path) -> Result<fs::File, ConfigError> {
    let root = fs::canonicalize(root).map_err(io)?;
    let path = root.join(".oxabl").join("cache.toml");
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ConfigError::MissingFile(path));
        }
        Err(e) => return Err(io(e)),
    };
    if !meta.is_file() {
        return Err(ConfigError::NotRegularFile(path));
    }
    let resolved = fs::canonicalize(&path).map_err(io)?;
    if !resolved.starts_with(&root) {
        return Err(ConfigError::NotRegularFile(path));
    }
    let file = fs::File::open(&path).map_err(io)?;
    if !file.metadata().map_err(io)?.is_file() {
        return Err(ConfigError::NotRegularFile(path));
    }
    Ok(file)
}

impl RepoFile {
    pub fn read(root: &Path) -> Result<Self, ConfigError> {
        let mut text = String::new();
        locate(root)?
            .take(MAX_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(io)?;
        if text.len() as u64 > MAX_BYTES {
            return Err(ConfigError::Invalid("cache.toml is too large"));
        }
        Self::parse(&text)
    }

    pub(crate) fn parse(text: &str) -> Result<Self, ConfigError> {
        let table: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| ConfigError::Parse(e.message().to_owned()))?;
        if let Some(key) = table.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(ConfigError::UnknownKey(key.clone()));
        }
        let raw: Raw = table
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError::Parse(e.message().to_owned()))?;
        let owner_keys = raw
            .owner_keys
            .iter()
            .map(|k| parse_owner_key(k))
            .collect::<Result<Vec<_>, _>>()?;
        if owner_keys.is_empty() {
            return Err(ConfigError::Invalid("owner_keys is empty"));
        }
        Ok(Self {
            server: Url::parse(&raw.server).map_err(|_| ConfigError::Invalid("server"))?,
            tenant: TenantId::from_text(&raw.tenant).map_err(|_| ConfigError::Invalid("tenant"))?,
            cache: CacheId::from_text(&raw.cache).map_err(|_| ConfigError::Invalid("cache"))?,
            owner_keys,
            min_roster_version: raw.min_roster_version,
        })
    }
}
