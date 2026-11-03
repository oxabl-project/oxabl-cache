//! The `[metadata]` section: where the metadata database lives.

use super::raw;
use std::path::PathBuf;

const DEFAULT_POOL: usize = 16;

/// The metadata database.
#[derive(Debug, Clone)]
pub(crate) enum Metadata {
    /// A SQLite file; the single process runs garbage collection.
    Sqlite(PathBuf),
    /// A Postgres database shared by several nodes.
    Postgres {
        url: String,
        password_file: Option<PathBuf>,
        ca_file: Option<PathBuf>,
        max_connections: usize,
    },
}

fn carries_password(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?']).next().unwrap_or("");
    let userinfo = authority.rsplit_once('@').map(|(userinfo, _)| userinfo);
    userinfo.is_some_and(|userinfo| userinfo.contains(':'))
        || url
            .split_once('?')
            .is_some_and(|(_, query)| query.split('&').any(|pair| pair.starts_with("password=")))
}

pub(super) fn parse(raw: &raw::Metadata) -> Result<Metadata, String> {
    let url = raw.url.as_str();
    if let Some(path) = url.strip_prefix("sqlite://") {
        if path.is_empty() {
            return Err("metadata url names no SQLite file".to_owned());
        }
        if raw.password_file.is_some() || raw.ca_file.is_some() || raw.max_connections.is_some() {
            return Err(
                "metadata.password_file, ca_file and max_connections apply to postgres only"
                    .to_owned(),
            );
        }
        return Ok(Metadata::Sqlite(PathBuf::from(path)));
    }
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        if carries_password(url) {
            return Err("the metadata url must not carry a password: use password_file".to_owned());
        }
        let max_connections = raw.max_connections.unwrap_or(DEFAULT_POOL);
        if max_connections == 0 {
            return Err("metadata.max_connections must be positive".to_owned());
        }
        return Ok(Metadata::Postgres {
            url: url.to_owned(),
            password_file: raw.password_file.as_ref().map(PathBuf::from),
            ca_file: raw.ca_file.as_ref().map(PathBuf::from),
            max_connections,
        });
    }
    Err(format!(
        "metadata url `{url}` is not sqlite://<path> or postgres://..."
    ))
}
