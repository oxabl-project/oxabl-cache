//! Identifiers placed in request paths.

use crate::error::Failure;

/// An id as a path segment: letters, digits and underscores only.
pub fn path_id(id: &str) -> Result<&str, Failure> {
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        Ok(id)
    } else {
        Err(Failure::usage(format!("invalid id '{id}'")))
    }
}
