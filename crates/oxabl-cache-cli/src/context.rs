//! Turns the global flags into the client's configuration.

use std::path::{Path, PathBuf};

use oxabl_cache_client::{
    Config, Credentials, IdentitySource, Mode, SecretString, default_identity_path,
};

use crate::cli::{Cli, ModeArg};
use crate::failure::Failure;
use crate::secrets;

/// The flags every command shares.
pub struct Context {
    repo: Option<PathBuf>,
    identity_file: Option<PathBuf>,
    passphrase_file: Option<PathBuf>,
    token_file: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    ca_file: Option<PathBuf>,
    mode: Option<Mode>,
    pub json: bool,
}

impl Context {
    pub fn new(cli: &Cli) -> Result<Self, Failure> {
        Ok(Self {
            repo: cli.repo.clone(),
            identity_file: cli.identity_file.clone(),
            passphrase_file: cli.passphrase_file.clone(),
            token_file: cli.token_file.clone(),
            state_dir: cli.state_dir.clone(),
            ca_file: cli.ca_file.clone(),
            mode: cli.mode.map(|mode| match mode {
                ModeArg::Ci => Mode::Ci,
                ModeArg::Developer => Mode::Developer,
                ModeArg::ReadOnly => Mode::ReadOnly,
            }),
            json: cli.json,
        })
    }

    pub fn state_dir(&self) -> Option<&Path> {
        self.state_dir.as_deref()
    }

    pub fn ca_file(&self) -> Option<&Path> {
        self.ca_file.as_deref()
    }

    /// The directory that holds `.oxabl/cache.toml`: `--repo`, or the nearest ancestor of the
    /// current directory that has one.
    fn repo_root(&self) -> Result<PathBuf, Failure> {
        if let Some(repo) = &self.repo {
            return Ok(repo.clone());
        }
        let start = std::env::current_dir().map_err(Failure::usage)?;
        start
            .ancestors()
            .find(|dir| dir.join(".oxabl").join("cache.toml").exists())
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                Failure::usage(
                    "no .oxabl/cache.toml here or above: run in the repository or pass --repo",
                )
            })
    }

    /// The identity file this invocation unlocks.
    pub fn identity_path(&self) -> Result<PathBuf, Failure> {
        let path = self
            .identity_file
            .clone()
            .or_else(default_identity_path)
            .ok_or_else(|| Failure::usage("no identity file: pass --identity-file"))?;
        if !path.is_file() {
            return Err(Failure::usage(format!(
                "no identity at {}: create one with `oxabl-cache identity new` or pass --identity-file",
                path.display()
            )));
        }
        Ok(path)
    }

    /// The passphrase source: the named file, else `OXABL_CACHE_IDENTITY_PASSPHRASE`, else the
    /// terminal.
    pub fn passphrase(&self) -> Result<Option<SecretString>, Failure> {
        match &self.passphrase_file {
            Some(path) => secrets::read_passphrase_file(path).map(Some),
            None => match secrets::environment_passphrase() {
                Some(passphrase) => Ok(Some(passphrase)),
                None if secrets::has_terminal() => Ok(None),
                None => Err(Failure::usage(
                    "no terminal to ask the passphrase on: pass --passphrase-file or set OXABL_CACHE_IDENTITY_PASSPHRASE",
                )),
            },
        }
    }

    /// The server credential: the named token file, else the library's own order, which starts
    /// with `OXABL_CACHE_TOKEN`.
    pub fn credentials(&self) -> Result<Credentials, Failure> {
        match &self.token_file {
            Some(path) => secrets::read_token_file(path).map(Credentials::Token),
            None => Ok(Credentials::Auto),
        }
    }

    /// The identity: the named file, else the identity string in `OXABL_CACHE_IDENTITY`, else
    /// the default identity file.
    fn identity_source(&self) -> Result<IdentitySource, Failure> {
        let from_environment =
            std::env::var_os("OXABL_CACHE_IDENTITY").is_some_and(|v| !v.is_empty());
        if self.identity_file.is_none() && from_environment {
            return Ok(IdentitySource::Auto);
        }
        self.identity_path().map(IdentitySource::File)
    }

    /// The repository's cache with this invocation's identity, credentials and settings.
    pub fn config(&self) -> Result<Config, Failure> {
        let source = self.identity_source()?;
        let protected = matches!(source, IdentitySource::File(_));
        let mut config = Config::from_repo(&self.repo_root()?)
            .map_err(Failure::usage)?
            .with_credentials(self.credentials()?)
            .with_identity(source);
        if protected {
            config = match self.passphrase()? {
                Some(passphrase) => config.with_passphrase(move || Some(passphrase.clone())),
                None => config.with_passphrase(|| secrets::prompt("Identity passphrase")),
            };
        }
        if let Some(mode) = self.mode {
            config = config.with_mode(mode);
        }
        if let Some(dir) = &self.state_dir {
            config = config.with_state_dir(dir.clone());
        }
        if let Some(ca) = &self.ca_file {
            config = config.with_ca_file(ca.clone());
        }
        Ok(config)
    }

    /// Like [`Context::config`] without an identity: for commands that only log in or out.
    pub fn session_config(&self) -> Result<Config, Failure> {
        let mut config = Config::from_repo(&self.repo_root()?).map_err(Failure::usage)?;
        if let Some(dir) = &self.state_dir {
            config = config.with_state_dir(dir.clone());
        }
        if let Some(ca) = &self.ca_file {
            config = config.with_ca_file(ca.clone());
        }
        Ok(config)
    }
}
