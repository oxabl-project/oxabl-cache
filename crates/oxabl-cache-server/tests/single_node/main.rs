//! The real `oxabl-cache-server` binary driven over raw HTTP on a fresh data
//! directory. Records and segments are random bytes of valid outer shape: the
//! server never opens them.

use support::objects::Kind;

/// The store this target runs against.
pub const STORE: Kind = Kind::Fs;

mod administration;
mod backup;
mod commit_retry;
mod config_rules;
mod dictionary_and_reports;
mod first_write_wins;
mod fs_support;
mod gc;
mod health;
mod lifecycle;
mod limits;
mod logs;
mod me_and_telemetry;
mod migrate;
mod object_urls;
mod operator_tokens;
mod roster_rules;
mod round_trip;
mod sample;
mod scopes;
mod secret_modes;
mod shutdown_upload;
mod store_root;
mod support;
mod teardown;
mod tls_listener;

use support::backend::Backend;

pub const BACKEND: Backend = Backend::Sqlite;
