//! Users.

use crate::error::Result;
use crate::ids::{PrincipalId, TenantId};

/// User rows.
pub trait UsersTx {
    /// Inserts a user; `false` if the id or the issuer and subject exist.
    fn insert_user(&mut self, user: &User) -> Result<bool>;
    /// A user by id.
    fn user(&self, id: &PrincipalId) -> Result<Option<User>>;
    /// Marks a user disabled; `false` if unknown or already disabled.
    fn disable_user(&mut self, id: &PrincipalId, now: i64) -> Result<bool>;
}

/// A human principal, keyed by issuer and subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// The principal id.
    pub id: PrincipalId,
    /// The tenant.
    pub tenant: TenantId,
    /// Issuer URL of the identity provider.
    pub issuer: String,
    /// Subject at the issuer.
    pub subject: String,
    /// Operator-visible label.
    pub label: String,
    /// Creation time, unix seconds.
    pub created_at: i64,
    /// Set when the user was disabled.
    pub disabled_at: Option<i64>,
}
