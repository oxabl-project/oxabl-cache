//! The `single_node` scenarios on Postgres metadata: the same modules, a different backend.

#[path = "../single_node/administration.rs"]
mod administration;
#[path = "../single_node/commit_retry.rs"]
mod commit_retry;
#[path = "../single_node/config_rules.rs"]
mod config_rules;
#[path = "../single_node/dictionary_and_reports.rs"]
mod dictionary_and_reports;
#[path = "../single_node/first_write_wins.rs"]
mod first_write_wins;
#[path = "../single_node/fs_support.rs"]
mod fs_support;
#[path = "../single_node/gc.rs"]
mod gc;
#[path = "../single_node/lifecycle.rs"]
mod lifecycle;
#[path = "../single_node/limits.rs"]
mod limits;
#[path = "../single_node/logs.rs"]
mod logs;
#[path = "../single_node/object_urls.rs"]
mod object_urls;
#[path = "../single_node/roster_rules.rs"]
mod roster_rules;
#[path = "../single_node/round_trip.rs"]
mod round_trip;
#[path = "../single_node/scopes.rs"]
mod scopes;
#[path = "../single_node/shutdown_upload.rs"]
mod shutdown_upload;
#[path = "../single_node/support/mod.rs"]
mod support;
#[path = "../single_node/tls_listener.rs"]
mod tls_listener;

mod scenario_list;

use support::backend::Backend;
use support::objects::Kind;

pub const BACKEND: Backend = Backend::Postgres;

/// The store this target runs against.
pub const STORE: Kind = Kind::Fs;
