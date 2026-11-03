//! The real `oxabl-cache-server` binary driven over raw HTTP on a fresh data
//! directory. Records and segments are random bytes of valid outer shape: the
//! server never opens them.

use support::objects::Kind;

/// The store this target runs against.
pub const STORE: Kind = Kind::Fs;

mod administration;
mod commit_retry;
mod config_rules;
mod dictionary_and_reports;
mod first_write_wins;
mod fs_support;
mod gc;
mod lifecycle;
mod limits;
mod logs;
mod object_urls;
mod roster_rules;
mod round_trip;
mod scopes;
mod shutdown_upload;
mod support;
mod tls_listener;

use support::backend::Backend;

pub const BACKEND: Backend = Backend::Sqlite;
