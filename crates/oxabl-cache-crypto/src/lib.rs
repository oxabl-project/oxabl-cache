//! Client-only cryptography: key hierarchy, lookup ids, seal and open, signatures, roster.
//!
//! Every function that takes bytes from outside treats them as hostile: it checks lengths
//! against the format limits before allocating and returns [`Error`] instead of panicking.
//! Secret types zeroize on drop and never print their contents. The server must never link
//! this crate.

mod aead;
mod compress;
mod dictionary_seal;
mod domain;
mod keyring;
mod nonce;
mod random;
mod secret;
mod signing;

pub mod dictionary;
pub mod dictionary_pointer;
pub mod error;
pub mod identity;
pub mod identity_env;
pub mod identity_file;
pub mod identity_string;
pub mod keys;
pub mod lookup;
pub mod object_keys;
pub mod record_context;
pub mod record_entry;
pub mod record_open;
pub mod record_seal;
pub mod roster_draft;
pub mod roster_member;
pub mod roster_verify;
pub mod segment_info;
pub mod segment_open;
pub mod segment_seal;
pub mod telemetry;
pub mod trust;

pub use age::secrecy::{ExposeSecret, SecretString};
pub use dictionary::{DEFAULT_DICTIONARY_BYTES, Dictionary, MAX_DICTIONARY_BYTES, train};
pub use dictionary_pointer::{sign_pointer, verify_pointer};
pub use dictionary_seal::{open_dictionary, seal_dictionary};
pub use error::{Error, Result};
pub use identity::Identity;
pub use keys::{LookupKey, RecordKey, RootKey, SegmentKey};
pub use object_keys::ObjectKeys;
pub use record_context::RecordContext;
pub use record_entry::{Entry, EntryFields, FileEntry};
pub use record_open::{OpenedRecord, open_record};
pub use record_seal::seal_record;
pub use roster_draft::RosterDraft;
pub use roster_member::MemberEntry;
pub use roster_verify::{RosterPins, VerifiedRoster};
pub use segment_info::{SegmentSeal, sealed_segment_len};
pub use segment_open::{SegmentExpectation, open_segment};
pub use segment_seal::{DEFAULT_ZSTD_LEVEL, SealOptions, seal_compressed, seal_segment};
pub use telemetry::{open_telemetry, seal_telemetry, telemetry_epoch};
pub use trust::{AcceptedRecord, Provenance};
