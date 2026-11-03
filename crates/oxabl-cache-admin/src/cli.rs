//! The command line: flags, subcommands and their help.

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

const EXIT_CODES: &str = "Exit codes: 0 ok, 1 the server refused (its error code and message are printed), \
2 usage or configuration error, 3 server unreachable.";

#[derive(Parser)]
#[command(name = "oxabl-cache-admin", version, about = "Operator command line for oxabl-cache-server", after_help = EXIT_CODES)]
pub struct Cli {
    /// Admin listener URL, such as http://127.0.0.1:9091.
    #[arg(long, global = true, env = "OXABL_CACHE_ADMIN_SERVER")]
    pub server: Option<String>,
    /// File holding the operator token. The token is never accepted on the command line.
    #[arg(
        long,
        global = true,
        env = "OXABL_CACHE_ADMIN_TOKEN_FILE",
        value_name = "PATH"
    )]
    pub token_file: Option<PathBuf>,
    /// PEM file of extra trusted CA certificates, added to the system roots.
    #[arg(long, global = true, value_name = "PATH")]
    pub ca_file: Option<PathBuf>,
    /// Print JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create, inspect and change tenants.
    #[command(subcommand)]
    Tenant(TenantCommand),
    /// Create tokens.
    #[command(subcommand)]
    Token(TokenCommand),
    /// Export usage.
    #[command(subcommand)]
    Usage(UsageCommand),
    /// Cache maintenance.
    #[command(subcommand)]
    Cache(CacheCommand),
    /// Report store, metadata and GC leader status.
    Health,
}

#[derive(Subcommand)]
pub enum TenantCommand {
    /// Create a tenant; the same name and limits again is a no-op.
    Create {
        name: String,
        /// Live-byte hard quota, such as 20GiB.
        #[arg(long)]
        hard_quota: Option<String>,
        /// Monthly egress quota, such as 500GiB.
        #[arg(long)]
        egress: Option<String>,
        /// Requests per second.
        #[arg(long)]
        rate: Option<u32>,
        /// Request burst size.
        #[arg(long)]
        burst: Option<u32>,
    },
    /// List every tenant.
    List,
    /// Show one tenant, by id or name.
    Show { tenant: String },
    /// Refuse every data request of a tenant until it is resumed.
    Suspend { tenant: String },
    /// Lift a suspension.
    Resume { tenant: String },
    /// Delete a tenant and its caches.
    Delete { tenant: String },
    /// Change a tenant's quotas; flags left out keep their value. Sizes accept `none`.
    Quota {
        tenant: String,
        #[arg(long)]
        hard_quota: Option<String>,
        #[arg(long)]
        egress: Option<String>,
        #[arg(long)]
        rate: Option<u32>,
        #[arg(long)]
        burst: Option<u32>,
    },
    /// Register the first human tenant administrator by OIDC issuer and subject.
    BootstrapAdmin {
        tenant: String,
        #[arg(long)]
        issuer: String,
        #[arg(long)]
        subject: String,
    },
}

#[derive(Subcommand)]
pub enum TokenCommand {
    /// Create a tenant token and print its secret once.
    Create(TokenCreate),
}

#[derive(Args)]
pub struct TokenCreate {
    /// Tenant id or name.
    #[arg(long)]
    pub tenant: String,
    /// Only `tenant:admin` is minted by the operator.
    #[arg(long, default_value = "tenant:admin")]
    pub scopes: String,
    /// Lifetime in whole days, such as 30d. Without it the server's default applies.
    #[arg(long)]
    pub expires: Option<String>,
    /// Label stored with the token.
    #[arg(long, default_value = "operator")]
    pub name: String,
}

#[derive(Subcommand)]
pub enum UsageCommand {
    /// Export one UTC month of usage.
    #[command(long_about = USAGE_HELP)]
    Export {
        /// Calendar month in UTC, YYYY-MM.
        #[arg(long)]
        month: String,
        #[arg(long, value_enum, default_value = "csv")]
        format: Format,
    },
}

const USAGE_HELP: &str = "Export one UTC month of usage, the input of the monthly billing script.\n\n\
CSV: the header `month,tenant,cache,metric,value`, then one row per tenant, cache and metric, \
sorted by tenant, cache, metric. `month` is YYYY-MM, `tenant` and `cache` are ids, `value` is an \
unsigned integer. Metrics: stored_byte_days, ingress_bytes, egress_bytes, requests, lookup_ids, \
hits, entries_committed, compaction_bytes.\n\n\
JSON: {\"month\": ..., \"usage\": [{tenant, cache, metric, value}, ...]} in the same order.";

#[derive(Clone, Copy, ValueEnum)]
pub enum Format {
    Csv,
    Json,
}

#[derive(Subcommand)]
pub enum CacheCommand {
    /// Re-verify every pack of a cache now.
    Scrub {
        /// Cache id, c_...
        cache: String,
    },
}
