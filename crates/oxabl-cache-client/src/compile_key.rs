//! The compile key oxabl computes for one compile unit.

use std::fmt;

/// BLAKE3 over a compile unit's canonical, root-relative inputs.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CompileKey(pub [u8; 32]);

impl fmt::Debug for CompileKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CompileKey(")?;
        for byte in &self.0[..6] {
            write!(f, "{byte:02x}")?;
        }
        f.write_str("…)")
    }
}
