use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::writer::Writer;

/// The version every frame carries.
pub const VERSION: u8 = 1;

/// Reads magic, version and flags. Returns the flags, which must lie within `allowed_flags`.
pub fn read_header(cursor: &mut Cursor<'_>, magic: [u8; 4], allowed_flags: u8) -> Result<u8> {
    if cursor.array::<4>()? != magic {
        return Err(FormatError::BadMagic);
    }
    let version = cursor.u8()?;
    if version != VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    let flags = cursor.u8()?;
    if flags & !allowed_flags != 0 {
        return Err(FormatError::UnknownFlags(flags));
    }
    Ok(flags)
}

/// Writes magic, version and flags.
pub fn write_header(writer: &mut Writer, magic: [u8; 4], flags: u8) {
    writer.bytes(&magic);
    writer.u8(VERSION);
    writer.u8(flags);
}
