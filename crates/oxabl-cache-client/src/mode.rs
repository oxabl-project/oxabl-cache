//! What a client does with the cache.

/// A client's role: which namespaces it reads and whether it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Reads trusted entries and writes trusted entries signed with its `ci` key.
    Ci,
    /// Reads trusted entries, then its own personal ones, and writes personal entries.
    Developer,
    /// Reads like a developer (personal entries only when it is a developer) and never writes.
    ReadOnly,
}

impl Mode {
    pub(crate) fn parse(text: &str) -> Option<Mode> {
        match text {
            "ci" => Some(Mode::Ci),
            "developer" => Some(Mode::Developer),
            "read-only" => Some(Mode::ReadOnly),
            _ => None,
        }
    }

    /// Whether this mode may create an uploader.
    pub fn writes(self) -> bool {
        !matches!(self, Mode::ReadOnly)
    }
}
