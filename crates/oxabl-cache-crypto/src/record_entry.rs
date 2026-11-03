//! The content of a record, as owned values.

use oxabl_cache_format::{EntryFile, EntryPayload, PublicKey, SegmentId, Signature};

/// One file of an artifact set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Relative artifact path.
    pub path: String,
    pub size: u64,
}

/// What a record says about a segment, apart from who signed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFields {
    pub segment_id: SegmentId,
    pub segment_len: u32,
    pub dict_id: Option<SegmentId>,
    /// Sum of the file sizes.
    pub plain_len: u64,
    pub created_at: i64,
    pub roster_version: u64,
    pub files: Vec<FileEntry>,
}

/// A record's content with the key that signed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub fields: EntryFields,
    pub signer: PublicKey,
}

impl EntryFields {
    pub(crate) fn payload(&self, signer: PublicKey, signature: Signature) -> EntryPayload<'_> {
        EntryPayload {
            segment_id: self.segment_id,
            segment_len: self.segment_len,
            dict_id: self.dict_id,
            plain_len: self.plain_len,
            created_at: self.created_at,
            roster_version: self.roster_version,
            signer,
            files: self
                .files
                .iter()
                .map(|f| EntryFile {
                    path: &f.path,
                    size: f.size,
                })
                .collect(),
            signature,
        }
    }

    pub(crate) fn from_payload(payload: &EntryPayload<'_>) -> Self {
        Self {
            segment_id: payload.segment_id,
            segment_len: payload.segment_len,
            dict_id: payload.dict_id,
            plain_len: payload.plain_len,
            created_at: payload.created_at,
            roster_version: payload.roster_version,
            files: payload
                .files
                .iter()
                .map(|f| FileEntry {
                    path: f.path.to_owned(),
                    size: f.size,
                })
                .collect(),
        }
    }
}
