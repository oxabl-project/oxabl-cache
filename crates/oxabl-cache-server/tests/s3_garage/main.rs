//! The store-independent scenarios and the S3 scenarios against Garage.

use support::backend::Backend;
use support::objects::Kind;

/// The store this target runs against.
pub const STORE: Kind = Kind::Garage;

/// The metadata these scenarios run on.
pub const BACKEND: Backend = Backend::Sqlite;

#[path = "../s3_suite/support.rs"]
mod support;

#[path = "../single_node/commit_retry.rs"]
mod commit_retry;
#[path = "../single_node/dictionary_and_reports.rs"]
mod dictionary_and_reports;
#[path = "../single_node/first_write_wins.rs"]
mod first_write_wins;
#[path = "../single_node/gc.rs"]
mod gc;
#[path = "../single_node/health.rs"]
mod health;
#[path = "../single_node/lifecycle.rs"]
mod lifecycle;
#[path = "../single_node/limits.rs"]
mod limits;
#[path = "../single_node/roster_rules.rs"]
mod roster_rules;
#[path = "../single_node/round_trip.rs"]
mod round_trip;
#[path = "../single_node/scopes.rs"]
mod scopes;
#[path = "../single_node/secret_modes.rs"]
mod secret_modes;
#[path = "../single_node/teardown.rs"]
mod teardown;

#[path = "../s3_suite/capabilities.rs"]
mod capabilities;
#[path = "../s3_suite/config.rs"]
mod config;
#[path = "../s3_suite/orphans.rs"]
mod orphans;
#[path = "../s3_suite/probe.rs"]
mod probe;
#[path = "../s3_suite/put_after_commit.rs"]
mod put_after_commit;
#[path = "../s3_suite/s3_support.rs"]
mod s3_support;
#[path = "../s3_suite/tamper.rs"]
mod tamper;
