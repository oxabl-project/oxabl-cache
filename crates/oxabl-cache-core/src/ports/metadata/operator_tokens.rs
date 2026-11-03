//! Operator credentials.

use crate::domain::credential::OperatorToken;
use crate::error::Result;
use crate::ids::CredentialId;

/// Queries and writes of operator tokens.
pub trait OperatorTokensTx {
    /// Inserts an operator token; `false` if the id exists.
    fn insert_operator_token(&mut self, token: &OperatorToken) -> Result<bool>;
    /// An operator token by id.
    fn operator_token(&self, id: &CredentialId) -> Result<Option<OperatorToken>>;
    /// Every operator token, oldest first.
    fn operator_tokens(&self) -> Result<Vec<OperatorToken>>;
    /// Whether any operator token was ever created.
    fn operator_tokens_exist(&self) -> Result<bool>;
    /// Revokes an operator token; `false` if it is unknown or already revoked.
    fn revoke_operator_token(&mut self, id: &CredentialId, now: i64) -> Result<bool>;
}
