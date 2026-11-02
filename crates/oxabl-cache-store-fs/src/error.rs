//! Mapping I/O failures onto the core error.

use oxabl_cache_core::Error;
use std::io;

/// A missing file is `NotFound`; every other I/O failure is `Unavailable`.
pub(crate) fn io_error(error: &io::Error) -> Error {
    if error.kind() == io::ErrorKind::NotFound {
        Error::NotFound
    } else {
        Error::unavailable(format!("object store: {error}"))
    }
}
