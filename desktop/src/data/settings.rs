use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::model::CacheCard;

/// Light, dark, or whatever the desktop prefers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

/// `desktop.toml`: the caches joined on this machine, by invite, and the settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default = "on")]
    pub effects: bool,
    #[serde(default)]
    pub invites: Vec<String>,
}

fn on() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            appearance: Appearance::System,
            effects: true,
            invites: Vec::new(),
        }
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    if let Some(dir) = path.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let staging = path.with_extension("tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&staging)?;
    io::Write::write_all(&mut file, bytes)?;
    file.sync_all()?;
    fs::rename(&staging, path)
}

impl Settings {
    /// The saved settings; a missing or unreadable file gives the defaults.
    pub fn load(path: &Path) -> Settings {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = toml::to_string(self).map_err(io::Error::other)?;
        write_private(path, text.as_bytes())
    }
}

/// The cards as the last run left them, so the window opens with content before any network.
pub fn load_snapshot(path: &Path) -> Vec<CacheCard> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_snapshot(path: &Path, cards: &[CacheCard]) -> io::Result<()> {
    let bytes = serde_json::to_vec(cards).map_err(io::Error::other)?;
    write_private(path, &bytes)
}
