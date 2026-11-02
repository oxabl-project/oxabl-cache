//! `OXEP` plaintext of a sealed record.

use crate::cursor::Cursor;
use crate::error::{FormatError, Result};
use crate::frame::{read_header, write_header};
use crate::ids::{PublicKey, SegmentId, Signature};
use crate::limits::{FILES_PER_SET, Limits, PACK_BYTES, PATH_BYTES, SET_PATH_BYTES};
use crate::record::{RECORD_HEADER_LEN, RECORD_TAG_LEN, padded_plaintext_len};
use crate::segment::MIN_SEGMENT_LEN;
use crate::writer::Writer;

const MAGIC: [u8; 4] = *b"OXEP";
const FIXED_LEN: usize = 132;
const SIGNATURE_LEN: usize = 64;
const FILE_MIN: usize = 2 + 8;
const ZERO_ID: [u8; 32] = [0; 32];

/// Smallest sealed record: header, tag, the fixed `OXEP` fields and the signature.
pub(crate) const MIN_SEALED_RECORD_LEN: usize =
    RECORD_HEADER_LEN + RECORD_TAG_LEN + FIXED_LEN + SIGNATURE_LEN;

/// One file of an artifact set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFile<'a> {
    /// Relative path: not empty, not absolute, no `..` component, no NUL.
    pub path: &'a str,
    pub size: u64,
}

/// The signed content of a record. [`EntryPayload::signed_bytes`] is the part the signature
/// covers, after the caller's domain-separated prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPayload<'a> {
    pub segment_id: SegmentId,
    pub segment_len: u32,
    pub dict_id: Option<SegmentId>,
    /// Sum of the file sizes.
    pub plain_len: u64,
    pub created_at: i64,
    pub roster_version: u64,
    pub signer: PublicKey,
    pub files: Vec<EntryFile<'a>>,
    pub signature: Signature,
}

fn check_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\0')
        || path.split('/').any(|part| part == "..")
    {
        return Err(FormatError::Invalid("path"));
    }
    if path.len() > PATH_BYTES {
        return Err(FormatError::LimitExceeded("path length"));
    }
    Ok(())
}

impl<'a> EntryPayload<'a> {
    fn check(&self, limits: &Limits) -> Result<()> {
        if self.files.len() > FILES_PER_SET {
            return Err(FormatError::LimitExceeded("files per set"));
        }
        let mut total_paths = 0usize;
        let mut total_size = 0u64;
        for (i, f) in self.files.iter().enumerate() {
            check_path(f.path)?;
            if self.files[..i].iter().any(|g| g.path == f.path) {
                return Err(FormatError::Invalid("duplicate path"));
            }
            total_paths += f.path.len();
            total_size = total_size
                .checked_add(f.size)
                .ok_or(FormatError::LimitExceeded("plaintext per artifact set"))?;
        }
        if total_paths > SET_PATH_BYTES {
            return Err(FormatError::LimitExceeded("path bytes per set"));
        }
        if total_size != self.plain_len {
            return Err(FormatError::Invalid(
                "plain_len differs from the file sizes",
            ));
        }
        if self.plain_len > limits.plaintext_bytes() {
            return Err(FormatError::LimitExceeded("plaintext per artifact set"));
        }
        let len = u64::from(self.segment_len);
        if len < MIN_SEGMENT_LEN as u64 || len > PACK_BYTES {
            return Err(FormatError::Invalid("segment_len"));
        }
        if self.dict_id.is_some_and(|d| d.0 == ZERO_ID) {
            return Err(FormatError::Invalid("zero dict_id"));
        }
        Ok(())
    }

    fn write_signed(&self, w: &mut Writer) -> Result<()> {
        write_header(w, MAGIC, 0);
        w.bytes(&self.segment_id.0);
        w.u32(self.segment_len);
        w.bytes(&self.dict_id.map_or(ZERO_ID, |d| d.0));
        w.u64(self.plain_len);
        w.i64(self.created_at);
        w.u64(self.roster_version);
        w.bytes(&self.signer);
        w.u16(self.files.len() as u16);
        for f in &self.files {
            w.str16(f.path)?;
            w.u64(f.size);
        }
        Ok(())
    }

    /// Every byte before the signature.
    pub fn signed_bytes(&self, limits: &Limits) -> Result<Vec<u8>> {
        self.check(limits)?;
        let mut w = Writer::default();
        self.write_signed(&mut w)?;
        Ok(w.into_vec())
    }

    /// The plaintext of the record: signed bytes, signature, zero padding so that the sealed
    /// record is a multiple of 256 bytes.
    pub fn encode(&self, limits: &Limits) -> Result<Vec<u8>> {
        self.check(limits)?;
        let mut w = Writer::default();
        self.write_signed(&mut w)?;
        w.bytes(&self.signature);
        let padded = padded_plaintext_len(w.len());
        w.zeros(padded - w.len());
        Ok(w.into_vec())
    }

    pub fn decode(bytes: &'a [u8], limits: &Limits) -> Result<Self> {
        let mut c = Cursor::new(bytes);
        read_header(&mut c, MAGIC, 0)?;
        let segment_id = SegmentId(c.array()?);
        let segment_len = c.u32()?;
        let dict: [u8; 32] = c.array()?;
        let plain_len = c.u64()?;
        let created_at = c.i64()?;
        let roster_version = c.u64()?;
        let signer = c.array()?;
        let file_count = c.count16(FILES_PER_SET, "files per set", FILE_MIN)?;
        let mut files = Vec::with_capacity(file_count);
        for _ in 0..file_count {
            files.push(EntryFile {
                path: c.str16()?,
                size: c.u64()?,
            });
        }
        let signature = c.array()?;
        let payload = Self {
            segment_id,
            segment_len,
            dict_id: (dict != ZERO_ID).then_some(SegmentId(dict)),
            plain_len,
            created_at,
            roster_version,
            signer,
            files,
            signature,
        };
        payload.check(limits)?;
        let used = bytes.len() - c.remaining();
        if bytes.len() != padded_plaintext_len(used) {
            return Err(FormatError::Invalid("padding length"));
        }
        if c.take(c.remaining())?.iter().any(|&b| b != 0) {
            return Err(FormatError::Invalid("padding is not zero"));
        }
        Ok(payload)
    }
}
