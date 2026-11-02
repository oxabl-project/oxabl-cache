//! API token text: `oxc_<id>_<secret>`.

use std::fmt;

use crate::base32;
use crate::error::{FormatError, Result};

const PREFIX: &str = "oxc_";

/// An API token: a public id and a bearer secret. The secret never appears in `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiToken {
    /// 10 bytes, 16 base32 characters.
    pub id: [u8; 10],
    /// 32 bytes, 52 base32 characters.
    pub secret: [u8; 32],
}

impl ApiToken {
    pub fn to_text(&self) -> String {
        format!(
            "{PREFIX}{}_{}",
            base32::encode(&self.id),
            base32::encode(&self.secret)
        )
    }

    pub fn from_text(s: &str) -> Result<Self> {
        let rest = s
            .strip_prefix(PREFIX)
            .ok_or(FormatError::Invalid("token prefix"))?;
        let (id, secret) = rest
            .split_once('_')
            .ok_or(FormatError::Invalid("token shape"))?;
        Ok(Self {
            id: base32::decode(id)?,
            secret: base32::decode(secret)?,
        })
    }
}

impl fmt::Debug for ApiToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ApiToken(oxc_{}_…)", base32::encode(&self.id))
    }
}
