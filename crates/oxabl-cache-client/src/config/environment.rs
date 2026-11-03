use std::ffi::OsString;

use super::error::ConfigError;
use crate::mode::Mode;

const TRUST_VARIABLES: [&str; 4] = [
    "OXABL_CACHE_TENANT",
    "OXABL_CACHE_CACHE",
    "OXABL_CACHE_OWNER_KEYS",
    "OXABL_CACHE_MIN_ROSTER_VERSION",
];

/// The variables `from_repo` reads.
pub(crate) struct Environment {
    pub server: Option<String>,
    pub mode: Mode,
}

fn text(name: &'static str) -> Result<Option<String>, ConfigError> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(ConfigError::Environment(name)),
    }
}

fn present(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

impl Environment {
    /// Reads the environment; a set trust variable is an error, never ignored.
    pub fn read() -> Result<Self, ConfigError> {
        if let Some(variable) = TRUST_VARIABLES.into_iter().find(|v| present(v).is_some()) {
            return Err(ConfigError::TrustOverride(variable));
        }
        let mode = match text("OXABL_CACHE_MODE")? {
            Some(value) => {
                Mode::parse(&value).ok_or(ConfigError::Environment("OXABL_CACHE_MODE"))?
            }
            None if present("CI").is_some() => Mode::ReadOnly,
            None => Mode::Developer,
        };
        Ok(Self {
            server: text("OXABL_CACHE_SERVER")?,
            mode,
        })
    }
}
