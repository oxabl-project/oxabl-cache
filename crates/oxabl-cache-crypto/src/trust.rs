//! Trust tiers: which signer a reader accepts for which namespace.

use oxabl_cache_format::{Namespace, PublicKey};

use crate::dictionary::Dictionary;
use crate::error::{Error, Result};
use crate::record_entry::Entry;
use crate::record_open::OpenedRecord;
use crate::roster_verify::VerifiedRoster;
use crate::segment_open::SegmentExpectation;

/// Where an accepted entry came from, for callers that keep a local copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provenance {
    pub tier: Namespace,
    pub signer: PublicKey,
    pub epoch: u32,
    pub roster_version: u64,
}

/// A record whose signer the current roster accepts for its namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedRecord {
    entry: Entry,
    provenance: Provenance,
}

impl OpenedRecord {
    /// Applies the trust tier rules under the current roster.
    ///
    /// A `trusted` record needs a signer that is a key of a `ci` member. A `personal` record
    /// needs the reader's own signing key, and the reader must be in the roster. A record
    /// that names a newer roster version than the current one is rejected.
    pub fn accept(self, roster: &VerifiedRoster, reader: &PublicKey) -> Result<AcceptedRecord> {
        let signer = self.entry.signer;
        let accepted = match self.context.namespace {
            Namespace::Trusted => roster.is_ci_key(&signer),
            Namespace::Personal => signer == *reader && roster.member_by_sign_key(reader).is_some(),
        };
        if !accepted {
            return Err(Error::SignerNotAccepted);
        }
        if self.entry.fields.roster_version > roster.version() {
            return Err(Error::RosterVersionAhead);
        }
        Ok(AcceptedRecord {
            provenance: Provenance {
                tier: self.context.namespace,
                signer,
                epoch: self.context.epoch,
                roster_version: self.entry.fields.roster_version,
            },
            entry: self.entry,
        })
    }
}

impl AcceptedRecord {
    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    pub fn provenance(&self) -> Provenance {
        self.provenance
    }

    /// What opening the record's segment must satisfy. `dictionary` must be the dictionary
    /// the record names, or `None` when it names none.
    pub fn segment_expectation<'a>(
        &self,
        dictionary: Option<&'a Dictionary>,
    ) -> Result<SegmentExpectation<'a>> {
        let named = self.entry.fields.dict_id;
        if named != dictionary.map(Dictionary::id) {
            return Err(Error::Mismatch("dictionary"));
        }
        Ok(SegmentExpectation {
            id: self.entry.fields.segment_id,
            sealed_len: u64::from(self.entry.fields.segment_len),
            plain_len: self.entry.fields.plain_len,
            dictionary,
        })
    }
}
