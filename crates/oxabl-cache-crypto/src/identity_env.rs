//! Loading an identity from the environment.

use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::identity::Identity;

/// Name of the variable that holds the identity string in CI.
pub const IDENTITY_VARIABLE: &str = "OXABL_CACHE_IDENTITY";

/// Reads and parses the identity string in `OXABL_CACHE_IDENTITY`.
pub fn from_env() -> Result<Identity> {
    from_env_var(IDENTITY_VARIABLE)
}

/// Reads and parses the identity string in the named variable.
pub fn from_env_var(name: &str) -> Result<Identity> {
    let value = Zeroizing::new(
        std::env::var(name).map_err(|_| Error::Identity("identity variable is not set"))?,
    );
    Identity::parse(&value)
}
