//! Byte formats: magic, versions, encode and decode, size limits. Handles no keys.
//!
//! Every decoder takes untrusted bytes, returns [`FormatError`] instead of panicking, checks
//! counts and lengths against [`limits`] before allocating, and rejects trailing bytes. Every
//! encoder emits the canonical form that its decoder accepts: decoding and re-encoding gives
//! the same bytes.

pub mod api_token;
pub mod base32;
pub mod bech32;
pub mod commit_request;
pub mod commit_response;
pub mod cursor;
pub mod dictionary_pointer;
pub mod entry_payload;
pub mod error;
pub mod frame;
pub mod ids;
pub mod limits;
mod location;
pub mod lookup_request;
pub mod lookup_response;
pub mod member_descriptor;
pub mod namespace;
pub mod pack_footer;
pub mod pack_header;
pub mod pack_trailer;
pub mod padme;
pub mod record;
pub mod roster;
pub mod segment;
pub mod segment_plaintext;
pub mod telemetry;
pub mod text_ids;
pub mod writer;

pub use api_token::ApiToken;
pub use commit_request::{CommitEntry, CommitRequest, CommitSegment, SegmentKind};
pub use commit_response::{CommitResponse, CommitStatus};
pub use dictionary_pointer::DictionaryPointer;
pub use entry_payload::{EntryFile, EntryPayload};
pub use error::{FormatError, Result};
pub use ids::{CacheId, LookupId, PackId, PublicKey, RequestId, SegmentId, Signature, TenantId};
pub use limits::Limits;
pub use lookup_request::{LookupFlags, LookupRequest, LookupShape};
pub use lookup_response::{HitRecord, LookupHit, LookupResponse, ResponseDictionary, ResponsePack};
pub use member_descriptor::MemberDescriptor;
pub use namespace::{Namespace, Namespaces};
pub use pack_footer::{PackFooter, PackSegment};
pub use pack_header::PackHeader;
pub use pack_trailer::PackTrailer;
pub use record::SealedRecord;
pub use roster::{Member, Role, Roster, RosterSignature};
pub use segment::{SealedSegment, SegmentHeader};
pub use segment_plaintext::{Codec, SegmentPlaintext};
pub use telemetry::{SealedTelemetry, TelemetryKind, TelemetryMode, TelemetryReport};
