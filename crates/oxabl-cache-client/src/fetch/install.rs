use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use oxabl_cache_crypto::{Dictionary, Error, open_segment};
use oxabl_cache_format::Limits;

use super::candidates::Candidate;
use super::types::{InstalledFile, MissReason};
use crate::cache::RosterView;
use crate::install::{Durability, SetWriter, Stage, StagedSet};

/// A candidate with the dictionary its record names, ready to be opened.
pub(crate) struct Ready {
    pub candidate: Candidate,
    pub dictionary: Option<Arc<Dictionary>>,
}

pub(crate) enum Failure {
    Miss(MissReason),
    /// The segment's bytes do not hash to its signed id.
    HashMismatch,
}

pub(crate) struct Settings<'a> {
    pub view: &'a RosterView,
    pub limits: &'a Limits,
    pub stage: &'a Stage,
    pub durability: Durability,
    pub cancel: &'a Arc<AtomicBool>,
}

/// What opening a segment leaves: files at their final paths, or verified temporary files
/// that the committer makes durable and renames.
pub(crate) enum Installed {
    Done(Vec<InstalledFile>),
    Staged(StagedSet),
}

/// Opens one segment into temporary files and, only once everything verified, either renames
/// them into place (`Durability::Caller`) or hands them over for a durable batch.
pub(crate) fn decode_install(
    settings: &Settings<'_>,
    ready: &Ready,
    sealed: &[u8],
) -> Result<Installed, Failure> {
    let expectation = ready
        .candidate
        .accepted
        .segment_expectation(ready.dictionary.as_deref())
        .map_err(|_| Failure::Miss(MissReason::Corrupt))?;
    let mut writer = SetWriter::create(
        settings.stage,
        &ready.candidate.files,
        settings.cancel.clone(),
    )
    .map_err(|e| Failure::Miss(MissReason::Install(e.kind())))?;
    open_segment(
        &settings.view.segment,
        sealed,
        &expectation,
        settings.limits,
        &mut writer,
    )
    .map_err(|e| match e {
        Error::SegmentId => Failure::HashMismatch,
        Error::Io(kind) => Failure::Miss(MissReason::Install(kind)),
        _ => Failure::Miss(MissReason::Corrupt),
    })?;
    let staged = writer
        .finish()
        .map_err(|_| Failure::Miss(MissReason::Corrupt))?;
    match settings.durability {
        Durability::PerFile => Ok(Installed::Staged(staged)),
        Durability::Caller => staged
            .commit(Durability::Caller)
            .map(Installed::Done)
            .map_err(|e| Failure::Miss(MissReason::Install(e.kind()))),
    }
}
