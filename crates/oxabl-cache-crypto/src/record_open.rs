//! Opening a record: commitment, AEAD, strict decoding and the signature over the transcript.

use oxabl_cache_format::{EntryPayload, Limits, Namespace, PublicKey, SealedRecord};
use zeroize::Zeroizing;

use crate::aead::Cipher;
use crate::error::{Error, Result};
use crate::keys::RecordKey;
use crate::nonce::RECORD_NONCE;
use crate::record_context::RecordContext;
use crate::record_entry::{Entry, EntryFields};
use crate::signing;

/// A record whose encryption and signature verified, but whose signer is not yet vetted
/// against a roster. Call [`OpenedRecord::accept`] to use it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedRecord {
    pub(crate) entry: Entry,
    pub(crate) context: RecordContext,
}

impl OpenedRecord {
    /// The key that signed the record.
    pub fn signer(&self) -> PublicKey {
        self.entry.signer
    }

    pub fn namespace(&self) -> Namespace {
        self.context.namespace
    }
}

/// Opens `sealed` for the slot `ctx`. Fails for any other cache, epoch, namespace kind or
/// lookup id, for any altered byte, and for a signature that does not verify.
pub fn open_record(
    key: &RecordKey,
    ctx: &RecordContext,
    sealed: &[u8],
    limits: &Limits,
) -> Result<OpenedRecord> {
    let record = SealedRecord::decode(sealed)?;
    let keys = key.object_keys(&record.salt);
    if !keys.commits_to(&record.k_commit) {
        return Err(Error::KeyCommitment);
    }
    let cipher = Cipher::new(keys.enc())?;
    let mut buf = Zeroizing::new(record.ciphertext.to_vec());
    let plaintext = cipher.open(RECORD_NONCE, &ctx.associated_data(), &mut buf)?;
    let payload = EntryPayload::decode(plaintext, limits)?;
    let transcript = ctx.entry_transcript(&payload.signed_bytes(limits)?);
    signing::verify(&payload.signer, &transcript, &payload.signature)?;
    Ok(OpenedRecord {
        entry: Entry {
            fields: EntryFields::from_payload(&payload),
            signer: payload.signer,
        },
        context: *ctx,
    })
}
