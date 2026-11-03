//! Lookup ids: the keyed hash of a compile key.

use oxabl_cache_format::LookupId;

use crate::keys::LookupKey;

impl LookupKey {
    /// `BLAKE3.keyed_hash(lookup_key, compile_key)`.
    pub fn lookup_id(&self, compile_key: &[u8; 32]) -> LookupId {
        LookupId(blake3::keyed_hash(self.bytes(), compile_key).into())
    }
}
