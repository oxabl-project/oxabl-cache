//! The signed pointer to a cache's dictionary segment.

use oxabl_cache_format::{CacheId, DictionaryPointer, SegmentId};

use crate::domain::{SIG_DICTIONARY, prefixed};
use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::roster_verify::VerifiedRoster;
use crate::signing;

/// Signs a pointer to `dict_segment_id` as `signer`.
pub fn sign_pointer(
    signer: &Identity,
    cache_id: CacheId,
    epoch: u32,
    version: u64,
    dict_segment_id: SegmentId,
) -> DictionaryPointer {
    let mut pointer = DictionaryPointer {
        cache_id,
        epoch,
        version,
        dict_segment_id,
        signer: signer.signing_public(),
        signature: [0; 64],
    };
    pointer.signature = signer.sign(&prefixed(SIG_DICTIONARY, &pointer.signed_bytes()));
    pointer
}

/// Verifies a pointer's signature and that its signer is a `ci` or `owner` key of the
/// current roster, for this cache and epoch.
pub fn verify_pointer(bytes: &[u8], roster: &VerifiedRoster) -> Result<DictionaryPointer> {
    let pointer = DictionaryPointer::decode(bytes)?;
    if pointer.cache_id != roster.cache_id() || pointer.epoch != roster.epoch() {
        return Err(Error::Mismatch("dictionary pointer cache or epoch"));
    }
    signing::verify(
        &pointer.signer,
        &prefixed(SIG_DICTIONARY, &pointer.signed_bytes()),
        &pointer.signature,
    )?;
    if !roster.is_ci_or_owner_key(&pointer.signer) {
        return Err(Error::SignerNotAccepted);
    }
    Ok(pointer)
}
