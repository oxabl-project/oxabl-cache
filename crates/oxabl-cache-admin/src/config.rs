//! Where the server is and how to authenticate: flags, environment and the token file.

use crate::error::Failure;
use std::fs;
use std::path::PathBuf;

pub struct Settings {
    pub server: String,
    pub token: String,
    pub ca_file: Option<PathBuf>,
}

impl Settings {
    pub fn load(
        server: Option<String>,
        token_file: Option<PathBuf>,
        ca_file: Option<PathBuf>,
    ) -> Result<Self, Failure> {
        let server = server.ok_or_else(|| {
            Failure::usage("no server: pass --server or set OXABL_CACHE_ADMIN_SERVER")
        })?;
        let server = server.trim_end_matches('/').to_owned();
        if !(server.starts_with("http://") || server.starts_with("https://")) {
            return Err(Failure::usage(format!(
                "invalid server '{server}': use an http:// or https:// URL"
            )));
        }
        let path = token_file.ok_or_else(|| Failure::usage("no token: pass --token-file"))?;
        let raw = fs::read_to_string(&path).map_err(|error| {
            Failure::usage(format!(
                "cannot read token file {}: {error}",
                path.display()
            ))
        })?;
        let token = raw.trim().to_owned();
        if token.is_empty() || token.contains(char::is_whitespace) {
            return Err(Failure::usage(format!(
                "token file {} must hold exactly one token",
                path.display()
            )));
        }
        Ok(Self {
            server,
            token,
            ca_file,
        })
    }
}
