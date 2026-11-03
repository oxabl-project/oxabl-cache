//! Shared input rules of issuers and policies.

use crate::error::{Error, Result};

const MAX_CLAIM_NAME: usize = 128;

/// A claim name is printable ASCII without spaces.
pub(crate) fn check_claim_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_CLAIM_NAME || !name.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(Error::bad_request(
            "a claim name is 1 to 128 printable ASCII characters",
        ));
    }
    Ok(())
}
