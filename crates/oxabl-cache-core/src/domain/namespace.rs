//! Where an entry lives.

use crate::ids::PrincipalId;

/// A namespace as stored: all zero for trusted, the owner's principal id otherwise.
pub type NamespaceKey = [u8; PrincipalId::LEN];

/// The kind of namespace a frame names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceKind {
    /// Written by CI.
    Trusted,
    /// Written by one principal for their own use.
    Personal,
}

/// A namespace of a cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namespace {
    /// Written only with `write:trusted`, readable by every reader.
    Trusted,
    /// Written and read only by its owner.
    Personal(PrincipalId),
}

impl Namespace {
    /// The stored form.
    pub const fn key(self) -> NamespaceKey {
        match self {
            Namespace::Trusted => [0; PrincipalId::LEN],
            Namespace::Personal(owner) => *owner.as_bytes(),
        }
    }

    /// Reads the stored form.
    pub fn from_key(key: NamespaceKey) -> Self {
        if key == [0; PrincipalId::LEN] {
            Namespace::Trusted
        } else {
            Namespace::Personal(PrincipalId::from_bytes(key))
        }
    }

    /// The kind of this namespace.
    pub const fn kind(self) -> NamespaceKind {
        match self {
            Namespace::Trusted => NamespaceKind::Trusted,
            Namespace::Personal(_) => NamespaceKind::Personal,
        }
    }

    /// The owner of a personal namespace.
    pub const fn owner(self) -> Option<PrincipalId> {
        match self {
            Namespace::Trusted => None,
            Namespace::Personal(owner) => Some(owner),
        }
    }
}
