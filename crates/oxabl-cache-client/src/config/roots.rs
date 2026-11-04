use std::fs::File;
use std::io::Read;
use std::path::Path;

use super::error::ConfigError;

const MAX_BYTES: u64 = 1024 * 1024;

/// The certificates of a PEM bundle, to trust in addition to the system roots. An unreadable
/// or empty bundle is an error, never an empty list.
pub(crate) fn load(path: Option<&Path>) -> Result<Vec<reqwest::Certificate>, ConfigError> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };
    let file = File::open(path).map_err(|e| ConfigError::Io(e.kind()))?;
    let meta = file.metadata().map_err(|e| ConfigError::Io(e.kind()))?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err(ConfigError::Invalid(
            "the CA file is not a regular file of at most 1 MiB",
        ));
    }
    let mut pem = Vec::new();
    file.take(MAX_BYTES)
        .read_to_end(&mut pem)
        .map_err(|e| ConfigError::Io(e.kind()))?;
    let roots = reqwest::Certificate::from_pem_bundle(&pem)
        .map_err(|_| ConfigError::Invalid("the CA file holds no valid PEM certificate"))?;
    if roots.is_empty() {
        return Err(ConfigError::Invalid("the CA file holds no certificate"));
    }
    Ok(roots)
}
