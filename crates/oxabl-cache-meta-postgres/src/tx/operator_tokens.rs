//! Operator credentials.

use super::{PgTx, RowExt, args};
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::credential::OperatorToken;
use oxabl_cache_core::ids::CredentialId;
use oxabl_cache_core::ports::metadata::OperatorTokensTx;
use tokio_postgres::Row;

const COLUMNS: &str = "credential_id, name, secret_hash, created_at, expires_at, revoked_at";

fn token_of(row: &Row) -> Result<OperatorToken> {
    Ok(OperatorToken {
        id: CredentialId::from_bytes(row.blob(0)?),
        name: row.text(1)?,
        secret_hash: row.blob(2)?,
        created_at: row.int(3)?,
        expires_at: row.int(4)?,
        revoked_at: row.opt_int(5)?,
    })
}

impl OperatorTokensTx for PgTx<'_> {
    fn insert_operator_token(&mut self, token: &OperatorToken) -> Result<bool> {
        let changed = self.execute(
            "INSERT INTO operator_tokens
               (credential_id, name, secret_hash, created_at, expires_at, revoked_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (credential_id) DO NOTHING",
            args![
                token.id.as_bytes().as_slice(),
                token.name,
                token.secret_hash.as_slice(),
                token.created_at,
                token.expires_at,
                token.revoked_at,
            ],
        )?;
        Ok(changed == 1)
    }

    fn operator_token(&self, id: &CredentialId) -> Result<Option<OperatorToken>> {
        self.query_opt(
            &format!("SELECT {COLUMNS} FROM operator_tokens WHERE credential_id = $1"),
            args![id.as_bytes().as_slice()],
        )?
        .as_ref()
        .map(token_of)
        .transpose()
    }

    fn operator_tokens(&self) -> Result<Vec<OperatorToken>> {
        self.query(
            &format!("SELECT {COLUMNS} FROM operator_tokens ORDER BY created_at, credential_id"),
            args![],
        )?
        .iter()
        .map(token_of)
        .collect()
    }

    fn revoke_operator_token(&mut self, id: &CredentialId, now: i64) -> Result<bool> {
        let changed = self.execute(
            "UPDATE operator_tokens SET revoked_at = $2
             WHERE credential_id = $1 AND revoked_at IS NULL",
            args![id.as_bytes().as_slice(), now],
        )?;
        Ok(changed == 1)
    }

    fn operator_tokens_exist(&self) -> Result<bool> {
        self.query_one("SELECT EXISTS (SELECT 1 FROM operator_tokens)", args![])?
            .flag(0)
    }
}
