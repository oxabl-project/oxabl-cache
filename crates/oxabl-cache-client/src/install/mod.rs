//! Atomic installation of an artifact set under a stage directory.

mod set_writer;
mod stage;

pub(crate) use set_writer::{SetWriter, StagedSet};
pub(crate) use stage::Stage;

/// When installed files reach stable storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Durability {
    /// Every file is durable before it gets its final name, and every directory before
    /// `Installed` is reported: sets are installed in batches behind one `syncfs`, with an
    /// fsync per file where `syncfs` fails or a file is on another device (the default).
    #[default]
    PerFile,
    /// No fsync; the caller makes the stage durable (oxabl: one `syncfs` before promotion).
    Caller,
}
