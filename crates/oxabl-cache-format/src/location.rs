//! Byte range of a segment inside a pack, as lookup responses report it.

use crate::error::{FormatError, Result};
use crate::limits::PACK_BYTES;
use crate::pack_header::PACK_HEADER_LEN;
use crate::segment::MIN_SEGMENT_LEN;

/// Checks that `[offset, offset + length)` is a plausible segment range inside a pack.
pub(crate) fn check_range(offset: u64, length: u32) -> Result<()> {
    let end = offset
        .checked_add(u64::from(length))
        .ok_or(FormatError::Invalid("segment range"))?;
    if offset < PACK_HEADER_LEN as u64 || u64::from(length) < MIN_SEGMENT_LEN as u64 {
        return Err(FormatError::Invalid("segment range"));
    }
    if end > PACK_BYTES {
        return Err(FormatError::LimitExceeded("pack size"));
    }
    Ok(())
}
