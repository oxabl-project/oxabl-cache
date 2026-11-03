//! Sealing a record: sign the transcript, pad to 256 bytes, encrypt as one AEAD message.

use oxabl_cache_format::{Limits, SealedRecord};
use zeroize::Zeroizing;

use crate::aead::Cipher;
use crate::error::Result;
use crate::identity::Identity;
use crate::keys::RecordKey;
use crate::nonce::RECORD_NONCE;
use crate::random;
use crate::record_context::RecordContext;
use crate::record_entry::EntryFields;

/// Signs `fields` as `signer` for the slot `ctx` and returns the sealed `OXER` bytes.
pub fn seal_record(
    key: &RecordKey,
    ctx: &RecordContext,
    fields: &EntryFields,
    signer: &Identity,
    limits: &Limits,
) -> Result<Vec<u8>> {
    let signer_public = signer.signing_public();
    let unsigned = fields.payload(signer_public, [0; 64]);
    let transcript = ctx.entry_transcript(&unsigned.signed_bytes(limits)?);
    let signature = signer.sign(&transcript);
    let plaintext = Zeroizing::new(fields.payload(signer_public, signature).encode(limits)?);

    let salt = random::array()?;
    let keys = key.object_keys(&salt);
    let cipher = Cipher::new(keys.enc())?;
    let mut ciphertext = Zeroizing::new(plaintext.to_vec());
    cipher.seal(RECORD_NONCE, &ctx.associated_data(), &mut ciphertext)?;
    Ok(SealedRecord {
        salt,
        k_commit: keys.commit(),
        ciphertext: &ciphertext,
    }
    .encode()?)
}
