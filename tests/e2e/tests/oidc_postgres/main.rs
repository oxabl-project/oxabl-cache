//! The `oidc` scenarios on Postgres metadata.

use oxabl_cache_e2e::cache_server::Metadata;

/// The metadata database this target runs on.
pub const BACKEND: Metadata = Metadata::Postgres;

#[path = "../oidc/bootstrap_cli.rs"]
mod bootstrap_cli;
#[path = "../oidc/exchange.rs"]
mod exchange;
#[path = "../oidc/logs.rs"]
mod logs;
#[path = "../oidc/policies.rs"]
mod policies;
#[path = "../oidc/rebinding.rs"]
mod rebinding;
#[path = "../oidc/rejections.rs"]
mod rejections;
#[path = "../oidc/ssrf.rs"]
mod ssrf;
#[path = "../oidc/support.rs"]
mod support;

mod scenario_list;
