//! Operator credentials, which belong to the service and to no tenant.

use crate::domain::credential::OperatorToken;
use crate::engine::Engine;
use crate::error::{Conflict, Error, Result};
use crate::ids::CredentialId;
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::time::DAY_SECS;
use crate::usecases::credential_text::{generate_operator, hash_secret, parse_operator};
use subtle::ConstantTimeEq;

const DEFAULT_OPERATOR_DAYS: i64 = 365;
const MAX_OPERATOR_DAYS: i64 = 3650;

/// A freshly created operator token. The text is shown once and never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOperatorToken {
    /// The credential text.
    pub secret: String,
    /// The public id.
    pub id: CredentialId,
    /// Expiry, unix seconds.
    pub expires_at: i64,
}

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Creates an operator token valid for `expires_in_secs` (a year by default).
    /// The caller is trusted by being the process that owns the database.
    pub async fn create_operator_token(
        &self,
        name: String,
        expires_in_secs: Option<i64>,
    ) -> Result<NewOperatorToken> {
        let lifetime = expires_in_secs.unwrap_or(DEFAULT_OPERATOR_DAYS * DAY_SECS);
        if name.is_empty() || lifetime <= 0 || lifetime > MAX_OPERATOR_DAYS * DAY_SECS {
            return Err(Error::bad_request(
                "operator token name or lifetime invalid",
            ));
        }
        let now = self.now();
        let (id, secret, text) = generate_operator();
        let token = OperatorToken {
            id,
            name,
            secret_hash: hash_secret(&secret),
            created_at: now,
            expires_at: now + lifetime,
            revoked_at: None,
        };
        let expires_at = token.expires_at;
        self.meta
            .transact(move |tx| {
                if tx.insert_operator_token(&token)? {
                    Ok(())
                } else {
                    Err(Error::Conflict(Conflict::Exists))
                }
            })
            .await?;
        Ok(NewOperatorToken {
            secret: text,
            id,
            expires_at,
        })
    }

    /// Every operator token, oldest first.
    pub async fn list_operator_tokens(&self) -> Result<Vec<OperatorToken>> {
        self.meta.read(|tx| tx.operator_tokens()).await
    }

    /// Revokes an operator token at once. The last token that is still valid cannot
    /// be revoked: the service would be left without an operator. An unknown or
    /// already revoked token is `NotFound`.
    pub async fn revoke_operator_token(&self, id: CredentialId) -> Result<()> {
        let now = self.now();
        self.meta
            .transact(move |tx| {
                let tokens = tx.operator_tokens()?;
                let valid =
                    |token: &&OperatorToken| token.revoked_at.is_none() && now < token.expires_at;
                let target = tokens
                    .iter()
                    .find(|token| token.id == id && token.revoked_at.is_none())
                    .ok_or(Error::NotFound)?;
                let others = tokens
                    .iter()
                    .filter(valid)
                    .filter(|token| token.id != target.id)
                    .count();
                if valid(&target) && others == 0 {
                    return Err(Error::Conflict(Conflict::LastOperatorToken));
                }
                if tx.revoke_operator_token(&id, now)? {
                    Ok(())
                } else {
                    Err(Error::NotFound)
                }
            })
            .await
    }

    /// Whether an operator token was ever created.
    pub async fn operator_tokens_exist(&self) -> Result<bool> {
        self.meta.read(|tx| tx.operator_tokens_exist()).await
    }

    /// Verifies an operator token. Every failure is `Unauthenticated`; the secret
    /// hash is compared in constant time.
    pub async fn authenticate_operator(&self, bearer: &str) -> Result<()> {
        let parsed = parse_operator(bearer).ok_or(Error::Unauthenticated)?;
        let now = self.now();
        let presented = hash_secret(&parsed.secret);
        let id = parsed.id;
        let token = self.meta.read(move |tx| tx.operator_token(&id)).await?;
        let stored = token.as_ref().map_or([0u8; 32], |token| token.secret_hash);
        let matches: bool = presented.ct_eq(&stored).into();
        match token {
            Some(token) if matches && token.revoked_at.is_none() && now < token.expires_at => {
                Ok(())
            }
            _ => Err(Error::Unauthenticated),
        }
    }
}
