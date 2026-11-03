//! Relative artifact paths and the rules every one of them satisfies.

use std::fmt;

use oxabl_cache_format::limits::PATH_BYTES;

/// Why a string is not an artifact path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathError {
    Empty,
    TooLong,
    Absolute,
    Backslash,
    Nul,
    DriveLetter,
    BadComponent,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "path is empty",
            Self::TooLong => "path is longer than 1,024 bytes",
            Self::Absolute => "path is absolute",
            Self::Backslash => "path contains a backslash",
            Self::Nul => "path contains a NUL",
            Self::DriveLetter => "path starts with a drive prefix",
            Self::BadComponent => "path has an empty, `.` or `..` component",
        })
    }
}

impl std::error::Error for PathError {}

/// A relative artifact path: UTF-8, `/`-separated, no empty, `.` or `..` components, no
/// leading `/`, no `\`, no NUL, no drive prefix, at most 1,024 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ArtifactPath(String);

impl ArtifactPath {
    pub fn new(path: &str) -> Result<ArtifactPath, PathError> {
        if path.is_empty() {
            return Err(PathError::Empty);
        }
        if path.len() > PATH_BYTES {
            return Err(PathError::TooLong);
        }
        if path.starts_with('/') {
            return Err(PathError::Absolute);
        }
        if path.contains('\\') {
            return Err(PathError::Backslash);
        }
        if path.contains('\0') {
            return Err(PathError::Nul);
        }
        let bytes = path.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return Err(PathError::DriveLetter);
        }
        if path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(PathError::BadComponent);
        }
        Ok(ArtifactPath(path.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ArtifactPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
