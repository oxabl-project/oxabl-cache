//! Local state under `$XDG_STATE_HOME/oxabl-cache`.

mod high_water;
mod session_file;

use std::path::{Path, PathBuf};

pub(crate) use high_water::HighWater;
pub(crate) use session_file::StoredSession;

fn root(override_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = override_dir {
        return Some(dir.to_path_buf());
    }
    let base = match std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME").filter(|v| !v.is_empty())?)
            .join(".local")
            .join("state"),
    };
    Some(base.join("oxabl-cache"))
}

/// The state directories of one client.
#[derive(Debug, Clone)]
pub(crate) struct StateDirs {
    root: Option<PathBuf>,
}

impl StateDirs {
    pub fn new(override_dir: Option<&Path>) -> Self {
        Self {
            root: root(override_dir),
        }
    }

    pub fn high_water(&self, cache: &str) -> HighWater {
        HighWater::new(self.root.as_ref().map(|root| root.join(cache)))
    }

    pub fn session(&self, tenant: &str) -> StoredSession {
        StoredSession::new(
            self.root
                .as_ref()
                .map(|root| root.join("session").join(tenant)),
        )
    }
}
