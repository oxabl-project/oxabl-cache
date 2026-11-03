//! OIDC exchange, trust policies and the issuer fetch guard, driven against the real
//! `oxabl-cache-server` binary and a fake identity provider inside a network namespace.

use oxabl_cache_e2e::cache_server::Metadata;

/// The metadata database this target runs on.
pub const BACKEND: Metadata = Metadata::Sqlite;

mod bootstrap_cli;
mod exchange;
mod logs;
mod policies;
mod rebinding;
mod rejections;
mod ssrf;
mod support;
