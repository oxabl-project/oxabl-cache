use std::io;
use std::path::PathBuf;
use std::time::Duration;

use oxabl_cache_crypto::Provenance as CryptoProvenance;
use oxabl_cache_format::{Namespace, PublicKey};

use crate::artifact_path::ArtifactPath;
use crate::compile_key::CompileKey;
use crate::install::Durability;

/// One compile key to fetch.
pub struct FetchRequest {
    pub key: CompileKey,
    /// If set, the entry's path set must equal this set exactly, or the outcome is `PathMismatch`.
    pub expect: Option<Vec<ArtifactPath>>,
}

/// How and where a fetch installs.
pub struct FetchOptions {
    /// Directory files are installed under (`stage/<artifact path>`).
    pub stage: PathBuf,
    pub durability: Durability,
    /// Ends the fetch when nothing progresses for this long; default 30 s.
    pub idle_timeout: Duration,
}

impl FetchOptions {
    /// Options for `stage` with per-file durability and the default idle timeout.
    pub fn new(stage: PathBuf) -> Self {
        Self {
            stage,
            durability: Durability::default(),
            idle_timeout: Duration::from_secs(30),
        }
    }
}

/// One installed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledFile {
    pub path: ArtifactPath,
    pub size: u64,
}

/// The namespace a hit came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Trusted,
    Personal,
}

/// Origin of a hit, for callers that keep a local copy; `signer` is the Ed25519 public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provenance {
    pub tier: Tier,
    pub signer: PublicKey,
    pub epoch: u32,
    pub roster_version: u64,
}

impl From<CryptoProvenance> for Provenance {
    fn from(p: CryptoProvenance) -> Self {
        Self {
            tier: match p.tier {
                Namespace::Trusted => Tier::Trusted,
                Namespace::Personal => Tier::Personal,
            },
            signer: p.signer,
            epoch: p.epoch,
            roster_version: p.roster_version,
        }
    }
}

/// Why a key is a miss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MissReason {
    /// The cache has no entry for the key.
    NotFound,
    /// The entry's signer is not accepted for its namespace, or the roster cannot judge it.
    Untrusted,
    /// Something failed verification: record, segment, dictionary or the server's answer.
    Corrupt,
    /// The entry's path set differs from the one expected.
    PathMismatch,
    /// Nothing progressed for the idle timeout.
    Timeout,
    /// The server or the object store could not be reached or answered an error.
    Unavailable,
    /// Installing into the stage failed.
    Install(io::ErrorKind),
}

/// The result for one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    Installed {
        key: CompileKey,
        files: Vec<InstalledFile>,
        provenance: Provenance,
    },
    Miss {
        key: CompileKey,
        reason: MissReason,
    },
}
