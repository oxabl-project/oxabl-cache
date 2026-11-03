//! Helpers for scenarios of the local object route only.

use crate::support::fixture::World;
use crate::support::http::{Response, request, split_url};

impl World {
    /// PUTs `bytes` to a bare object URL, with no headers beyond the length.
    pub fn put_object(&self, url: &str, bytes: &[u8]) -> Response {
        let (addr, path) = split_url(url);
        request(&addr, "PUT", &path, &[], bytes)
    }
}
