//! Text forms of ids: `c_` and `t_` ids, and request ids.

use crate::base32;
use crate::error::{FormatError, Result};
use crate::ids::{CacheId, RequestId, TenantId};

fn with_prefix(prefix: &str, bytes: &[u8]) -> String {
    format!("{prefix}{}", base32::encode(bytes))
}

fn strip<'a>(prefix: &str, s: &'a str) -> Result<&'a str> {
    s.strip_prefix(prefix)
        .ok_or(FormatError::Invalid("id prefix"))
}

impl CacheId {
    /// `c_` and 26 base32 characters.
    pub fn to_text(&self) -> String {
        with_prefix("c_", &self.0)
    }

    pub fn from_text(s: &str) -> Result<Self> {
        base32::decode(strip("c_", s)?).map(Self)
    }
}

impl TenantId {
    /// `t_` and 26 base32 characters.
    pub fn to_text(&self) -> String {
        with_prefix("t_", &self.0)
    }

    pub fn from_text(s: &str) -> Result<Self> {
        base32::decode(strip("t_", s)?).map(Self)
    }
}

impl RequestId {
    /// 26 base32 characters.
    pub fn to_text(&self) -> String {
        base32::encode(&self.0)
    }

    pub fn from_text(s: &str) -> Result<Self> {
        base32::decode(s).map(Self)
    }
}
