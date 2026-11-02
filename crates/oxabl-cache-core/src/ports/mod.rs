//! Port traits: everything the core needs from the outside world.

pub mod clock;
pub mod identity;
pub mod metadata;
pub mod object_store;
pub mod pack_assembler;

pub use clock::{Clock, SystemClock};
pub use identity::{IdentityVerifier, VerifiedIdentity};
pub use metadata::{MetaTx, MetadataStore};
pub use object_store::{
    ObjectHead, ObjectInfo, ObjectKey, ObjectStore, SignedUrl, StoreCapabilities,
};
pub use pack_assembler::{AssembledPack, PackAssembler, PackPart, Placement};
