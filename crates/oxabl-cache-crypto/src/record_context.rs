//! What a record is bound to: cache, epoch, namespace kind and lookup id.

use oxabl_cache_format::{CacheId, LookupId, Namespace};

use crate::domain::{AD_RECORD, SIG_ENTRY, prefixed};

/// The identity of a record slot. A record opens only under the context it was sealed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordContext {
    pub cache_id: CacheId,
    pub epoch: u32,
    pub namespace: Namespace,
    pub lookup_id: LookupId,
}

impl RecordContext {
    fn tail(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + 4 + 1 + 32);
        out.extend_from_slice(&self.cache_id.0);
        out.extend_from_slice(&self.epoch.to_be_bytes());
        out.push(self.namespace.to_u8());
        out.extend_from_slice(&self.lookup_id.0);
        out
    }

    /// `"oxabl-cache v1 record" ‖ cache_id ‖ epoch ‖ ns_kind ‖ lookup_id`.
    pub(crate) fn associated_data(&self) -> Vec<u8> {
        prefixed(AD_RECORD, &self.tail())
    }

    /// The signed transcript of an entry: its domain prefix, this context and the payload.
    pub(crate) fn entry_transcript(&self, signed_payload: &[u8]) -> Vec<u8> {
        let mut out = prefixed(SIG_ENTRY, &self.tail());
        out.extend_from_slice(signed_payload);
        out
    }
}
