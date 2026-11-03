//! Single-use keys of one sealed object, derived from a sub-key and the object's salt.

use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::domain::{CTX_COMMIT, CTX_ENC};
use crate::keys::{RecordKey, SegmentKey};

/// `K_enc` and `K_commit` of one object.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct ObjectKeys {
    enc: [u8; 32],
    commit: [u8; 32],
}

impl ObjectKeys {
    fn derive(sub_key: &[u8; 32], salt: &[u8; 32]) -> Self {
        let mut material = Zeroizing::new([0u8; 64]);
        material[..32].copy_from_slice(sub_key);
        material[32..].copy_from_slice(salt);
        Self {
            enc: blake3::derive_key(CTX_ENC, material.as_slice()),
            commit: blake3::derive_key(CTX_COMMIT, material.as_slice()),
        }
    }

    pub(crate) fn enc(&self) -> &[u8; 32] {
        &self.enc
    }

    pub(crate) fn commit(&self) -> [u8; 32] {
        self.commit
    }

    /// Compares a header's commitment with this object's, in constant time.
    pub(crate) fn commits_to(&self, header_commit: &[u8; 32]) -> bool {
        bool::from(self.commit.ct_eq(header_commit))
    }
}

impl std::fmt::Debug for ObjectKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ObjectKeys(<redacted>)")
    }
}

impl SegmentKey {
    /// Keys of the segment sealed under `salt`.
    pub fn object_keys(&self, salt: &[u8; 32]) -> ObjectKeys {
        ObjectKeys::derive(self.bytes(), salt)
    }
}

impl RecordKey {
    /// Keys of the record sealed under `salt`.
    pub fn object_keys(&self, salt: &[u8; 32]) -> ObjectKeys {
        ObjectKeys::derive(self.bytes(), salt)
    }
}
