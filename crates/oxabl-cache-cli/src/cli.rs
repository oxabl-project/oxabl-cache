//! The command line: flags, subcommands and their help.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

const EXIT_CODES: &str = "Exit codes: 0 done, 1 refused or failed, 2 usage or configuration error. \
Secrets are read from a file named by a flag, from OXABL_CACHE_TOKEN, OXABL_CACHE_IDENTITY and \
OXABL_CACHE_IDENTITY_PASSPHRASE, or for a passphrase from the terminal, never from arguments. A file named by a flag wins over the variable.";

#[derive(Parser)]
#[command(name = "oxabl-cache", version, about = "The oxabl-cache member command line", after_help = EXIT_CODES)]
pub struct Cli {
    /// The repository whose .oxabl/cache.toml names the cache; default: the nearest one above
    /// the current directory.
    #[arg(long, global = true, value_name = "DIR")]
    pub repo: Option<PathBuf>,
    /// The member identity file; default: the identity string in OXABL_CACHE_IDENTITY, else
    /// $XDG_CONFIG_HOME/oxabl-cache/identity.
    #[arg(long, global = true, value_name = "PATH")]
    pub identity_file: Option<PathBuf>,
    /// A file holding the identity file's passphrase; default: OXABL_CACHE_IDENTITY_PASSPHRASE,
    /// else the passphrase is asked on the terminal.
    #[arg(long, global = true, value_name = "PATH")]
    pub passphrase_file: Option<PathBuf>,
    /// A file holding an API token; default: OXABL_CACHE_TOKEN, else the stored login session.
    #[arg(long, global = true, value_name = "PATH")]
    pub token_file: Option<PathBuf>,
    /// Where roster state and the login session live; default: $XDG_STATE_HOME/oxabl-cache.
    #[arg(long, global = true, value_name = "DIR")]
    pub state_dir: Option<PathBuf>,
    /// PEM file of extra trusted CA certificates, added to the system roots.
    #[arg(long, global = true, value_name = "PATH")]
    pub ca_file: Option<PathBuf>,
    /// ci, developer or read-only; default: read-only when CI is set, else developer.
    #[arg(long, global = true, value_enum)]
    pub mode: Option<ModeArg>,
    /// Print JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ModeArg {
    Ci,
    Developer,
    ReadOnly,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum RoleArg {
    Ci,
    Developer,
    Reader,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create and inspect member identities.
    #[command(subcommand)]
    Identity(IdentityCommand),
    /// Create the first roster of a cache that exists with this owner's key registered.
    Init(InitArgs),
    /// Add or remove members.
    #[command(subcommand)]
    Member(MemberCommand),
    /// Start a new epoch for all current members.
    Rotate {
        /// Mark every entry of an older epoch dead.
        #[arg(long)]
        purge: bool,
    },
    /// Change the owners of the roster.
    #[command(subcommand)]
    Owner(OwnerCommand),
    /// Print a one-line invite that lets a person join this cache from the desktop app.
    Invite(InviteArgs),
    /// Log in with a device code and store a session.
    Login(LoginArgs),
    /// Revoke the stored session and delete it.
    Logout,
    /// Show the mode, the principal, the roster, the epoch and the quota, or why the cache is off.
    Status,
    /// Train and publish the compression dictionary.
    #[command(subcommand)]
    Dict(DictCommand),
    /// Fetch and fully verify a random sample of trusted entries.
    Verify(VerifyArgs),
    /// Fetch one key into a stage directory.
    Fetch(FetchArgs),
    /// Store one key from files on disk.
    Put(PutArgs),
}

#[derive(Subcommand)]
pub enum IdentityCommand {
    /// Create an identity and print its public descriptor.
    New(IdentityNewArgs),
    /// Keep the identity file's passphrase in the operating system's keyring, so nothing asks
    /// for it again.
    Remember,
    /// Remove the identity file's passphrase from the keyring.
    Forget,
}

#[derive(Args)]
pub struct IdentityNewArgs {
    /// Where the passphrase-protected identity file goes; default: the identity file path.
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,
    /// Write the identity string to this file instead, for a CI secret store; no passphrase
    /// file is made unless --file is given too.
    #[arg(long, value_name = "PATH")]
    pub secret_out: Option<PathBuf>,
}

#[derive(Args)]
pub struct InitArgs {
    /// The cache server URL.
    #[arg(long, env = "OXABL_CACHE_SERVER")]
    pub server: String,
    /// The tenant id, t_….
    #[arg(long)]
    pub tenant: String,
    /// The cache id, c_….
    #[arg(long)]
    pub cache: String,
    /// The owner's label in the roster.
    #[arg(long)]
    pub label: String,
}

#[derive(Subcommand)]
pub enum MemberCommand {
    /// Add a member in the current epoch.
    Add {
        /// The member's public descriptor, from `identity new`.
        descriptor: String,
        #[arg(long)]
        label: String,
        #[arg(long, value_enum)]
        role: RoleArg,
    },
    /// Remove a member and start a new epoch without them.
    Remove {
        label: String,
        /// Mark every entry of an older epoch dead.
        #[arg(long)]
        purge: bool,
    },
}

#[derive(Subcommand)]
pub enum OwnerCommand {
    /// Add an owner; their identity cosigns the roster.
    Add {
        /// The new owner's public descriptor.
        descriptor: String,
        #[arg(long)]
        label: String,
        /// The new owner's identity file, whose key co-signs the roster.
        #[arg(long, value_name = "PATH")]
        cosign_identity_file: PathBuf,
        /// A file holding the passphrase of the cosigning identity file.
        #[arg(long, value_name = "PATH")]
        cosign_passphrase_file: Option<PathBuf>,
    },
    /// Remove an owner, by label or `ed25519:` key, and start a new epoch without them.
    Remove {
        owner: String,
        /// Mark every entry of an older epoch dead.
        #[arg(long)]
        purge: bool,
    },
}

#[derive(Args)]
pub struct InviteArgs {
    /// The name the invite shows, such as the codebase.
    #[arg(long)]
    pub name: String,
    /// The identity provider's issuer URL people sign in at. Without it and `--client-id`,
    /// people join with an access token instead.
    #[arg(long, env = "OXABL_CACHE_OIDC_ISSUER", requires = "client_id")]
    pub issuer: Option<String>,
    /// The public client id of the device login.
    #[arg(long, env = "OXABL_CACHE_OIDC_CLIENT_ID", requires = "issuer")]
    pub client_id: Option<String>,
}

#[derive(Args)]
pub struct LoginArgs {
    /// The identity provider's issuer URL.
    #[arg(long, env = "OXABL_CACHE_OIDC_ISSUER")]
    pub issuer: String,
    /// The client id registered with the issuer for the device grant.
    #[arg(long, env = "OXABL_CACHE_OIDC_CLIENT_ID")]
    pub client_id: String,
}

#[derive(Subcommand)]
pub enum DictCommand {
    /// Train a dictionary from a directory of outputs and report what it gains.
    Train {
        #[arg(long, value_name = "DIR")]
        outputs: PathBuf,
        /// Publish it as the cache's dictionary when it gains at least 1.3x; needs ci mode.
        #[arg(long)]
        publish: bool,
    },
}

#[derive(Args)]
pub struct VerifyArgs {
    /// How many entries to fetch; at most 1,000 unless --keys is given.
    #[arg(long, value_name = "N")]
    pub sample: usize,
    /// A file of compile keys, one 64-digit hex key per line, to draw the sample from; without
    /// it the server names the sample.
    #[arg(long, value_name = "PATH")]
    pub keys: Option<PathBuf>,
}

#[derive(Args)]
pub struct FetchArgs {
    /// The compile key, 64 hex digits.
    #[arg(long)]
    pub key: String,
    /// The directory to install into.
    #[arg(long, value_name = "DIR")]
    pub stage: PathBuf,
    /// An artifact path the entry must hold; repeat for each. The entry's paths must equal the set.
    #[arg(long, value_name = "PATH")]
    pub expect: Vec<String>,
}

#[derive(Args)]
pub struct PutArgs {
    /// The compile key, 64 hex digits.
    #[arg(long)]
    pub key: String,
    /// `<artifact path>=<file on disk>`; repeat for each file of the set.
    #[arg(long = "file", value_name = "ARTIFACT=PATH", required = true)]
    pub files: Vec<String>,
}
