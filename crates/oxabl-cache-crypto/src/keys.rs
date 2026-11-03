//! The cache root key of an epoch and the sub-keys derived from it.

use crate::domain::{CTX_LOOKUP, CTX_RECORD, CTX_SEGMENT};
use crate::error::Result;
use crate::random;
use crate::secret::secret_key;

secret_key!(
    /// Cache root key `CRK_e`: 32 random bytes, one per epoch, held only by members.
    RootKey
);
secret_key!(
    /// Key of keyed lookup ids, derived from the root key.
    LookupKey
);
secret_key!(
    /// Parent of the per-segment keys, derived from the root key.
    SegmentKey
);
secret_key!(
    /// Parent of the per-record keys, derived from the root key.
    RecordKey
);

impl RootKey {
    /// A fresh key from the operating system's random generator.
    pub fn generate() -> Result<Self> {
        random::array().map(Self)
    }

    pub fn lookup_key(&self) -> LookupKey {
        LookupKey(blake3::derive_key(CTX_LOOKUP, &self.0))
    }

    pub fn segment_key(&self) -> SegmentKey {
        SegmentKey(blake3::derive_key(CTX_SEGMENT, &self.0))
    }

    pub fn record_key(&self) -> RecordKey {
        RecordKey(blake3::derive_key(CTX_RECORD, &self.0))
    }
}
