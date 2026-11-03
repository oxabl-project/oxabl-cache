use std::fmt;
use std::io;
use std::path::PathBuf;

/// Why a configuration is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigError {
    /// `.oxabl/cache.toml` does not exist.
    MissingFile(PathBuf),
    /// `.oxabl/cache.toml` is not a regular file inside the repository root.
    NotRegularFile(PathBuf),
    /// The file could not be read.
    Io(io::ErrorKind),
    /// The file is not valid TOML of the expected shape.
    Parse(String),
    /// The file has a key other than the five it defines.
    UnknownKey(String),
    /// A value is out of range or malformed.
    Invalid(&'static str),
    /// The environment tries to override a trust field, named here.
    TrustOverride(&'static str),
    /// An environment variable holds an unusable value.
    Environment(&'static str),
    /// The server URL is not `https`, or `http` on a loopback address.
    InsecureServer,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFile(path) => write!(f, "{} does not exist", path.display()),
            Self::NotRegularFile(path) => write!(
                f,
                "{} is not a regular file inside the repository",
                path.display()
            ),
            Self::Io(kind) => write!(f, "cannot read the cache configuration: {kind}"),
            Self::Parse(message) => write!(f, "invalid cache configuration: {message}"),
            Self::UnknownKey(key) => write!(f, "unknown key `{key}` in the cache configuration"),
            Self::Invalid(what) => write!(f, "invalid cache configuration: {what}"),
            Self::TrustOverride(variable) => write!(
                f,
                "{variable} cannot override a trust field of .oxabl/cache.toml"
            ),
            Self::Environment(variable) => write!(f, "{variable} holds an unusable value"),
            Self::InsecureServer => {
                f.write_str("the server URL must be https, or http on a loopback address")
            }
        }
    }
}

impl std::error::Error for ConfigError {}
