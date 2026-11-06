use std::path::PathBuf;

/// Where the app keeps what it keeps. Production uses the XDG directories the command line uses
/// too; tests point every one of them at a directory of their own.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `desktop.toml`: the joined caches and the settings.
    pub config_file: PathBuf,
    /// The client library's state directory: sessions, journals, roster marks.
    pub state_dir: PathBuf,
    /// The member identity file.
    pub identity_file: PathBuf,
    /// A private CA bundle trusted besides the system roots, from `OXABL_CACHE_CA_FILE`.
    pub ca_file: Option<PathBuf>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn xdg(variable: &str, fallback: &[&str]) -> PathBuf {
    std::env::var_os(variable)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback.iter().fold(home(), |path, part| path.join(part)))
}

impl Paths {
    pub fn from_environment() -> Paths {
        let config = xdg("XDG_CONFIG_HOME", &[".config"]).join("oxabl-cache");
        Paths {
            config_file: config.join("desktop.toml"),
            state_dir: oxabl_cache_client::default_state_dir()
                .unwrap_or_else(|| xdg("XDG_STATE_HOME", &[".local", "state"]).join("oxabl-cache")),
            identity_file: oxabl_cache_client::default_identity_path()
                .unwrap_or_else(|| config.join("identity")),
            ca_file: std::env::var_os("OXABL_CACHE_CA_FILE")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
        }
    }

    /// Everything under one directory.
    pub fn under(root: &std::path::Path) -> Paths {
        Paths {
            config_file: root.join("config").join("desktop.toml"),
            state_dir: root.join("state"),
            identity_file: root.join("config").join("identity"),
            ca_file: None,
        }
    }

    pub fn snapshot_file(&self) -> PathBuf {
        self.state_dir.join("desktop-snapshot.json")
    }
}
