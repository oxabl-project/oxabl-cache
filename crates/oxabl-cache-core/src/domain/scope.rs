//! Scopes and grants.

use crate::ids::CacheId;

/// A permission a principal holds on a cache, or on the tenant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Look up and fetch entries.
    Read,
    /// Write one's own personal namespace.
    WritePersonal,
    /// Write the trusted namespace.
    WriteTrusted,
    /// Manage the cache: roster, purge.
    CacheAdmin,
    /// Manage the tenant: caches, tokens, grants.
    TenantAdmin,
}

impl Scope {
    const ALL: [Scope; 5] = [
        Scope::Read,
        Scope::WritePersonal,
        Scope::WriteTrusted,
        Scope::CacheAdmin,
        Scope::TenantAdmin,
    ];

    const fn bit(self) -> u8 {
        match self {
            Scope::Read => 1,
            Scope::WritePersonal => 2,
            Scope::WriteTrusted => 4,
            Scope::CacheAdmin => 8,
            Scope::TenantAdmin => 16,
        }
    }

    /// The protocol name of the scope.
    pub const fn name(self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::WritePersonal => "write:personal",
            Scope::WriteTrusted => "write:trusted",
            Scope::CacheAdmin => "cache:admin",
            Scope::TenantAdmin => "tenant:admin",
        }
    }

    /// Parses a protocol scope name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scope| scope.name() == name)
    }
}

/// A set of scopes, stored as a bit mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ScopeSet(u8);

impl ScopeSet {
    /// The empty set.
    pub const EMPTY: ScopeSet = ScopeSet(0);

    /// Reads a stored mask, rejecting unknown bits.
    pub fn from_bits(bits: u8) -> Option<Self> {
        let known = Scope::ALL.iter().fold(0, |mask, scope| mask | scope.bit());
        (bits & !known == 0).then_some(Self(bits))
    }

    /// The stored mask.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether the set holds `scope`.
    pub const fn contains(self, scope: Scope) -> bool {
        self.0 & scope.bit() != 0
    }

    /// The set with `scope` added.
    #[must_use]
    pub const fn with(self, scope: Scope) -> Self {
        Self(self.0 | scope.bit())
    }

    /// Whether the set is empty.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl FromIterator<Scope> for ScopeSet {
    fn from_iter<I: IntoIterator<Item = Scope>>(scopes: I) -> Self {
        scopes
            .into_iter()
            .fold(ScopeSet::EMPTY, |set, scope| set.with(scope))
    }
}

/// Scopes on one cache, or tenant-wide when `cache` is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant {
    /// The cache the scopes apply to, or `None` for the whole tenant.
    pub cache: Option<CacheId>,
    /// The scopes granted.
    pub scopes: ScopeSet,
}
