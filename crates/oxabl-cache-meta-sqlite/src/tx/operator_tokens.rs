//! Operator credentials.

use super::SqliteTx;
use crate::db::{DbResult, RowExt};
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::credential::OperatorToken;
use oxabl_cache_core::ids::CredentialId;
use oxabl_cache_core::ports::metadata::OperatorTokensTx;
use rusqlite::{OptionalExtension, Row, params};

const COLUMNS: &str = "credential_id, name, secret_hash, created_at, expires_at, revoked_at";

fn token_of(row: &Row<'_>) -> rusqlite::Result<OperatorToken> {
    Ok(OperatorToken {
        id: CredentialId::from_bytes(row.blob(0)?),
        name: row.get(1)?,
        secret_hash: row.blob(2)?,
        created_at: row.get(3)?,
        expires_at: row.get(4)?,
        revoked_at: row.get(5)?,
    })
}

impl OperatorTokensTx for SqliteTx<'_> {
    fn insert_operator_token(&mut self, token: &OperatorToken) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(
                "INSERT INTO operator_tokens
                   (credential_id, name, secret_hash, created_at, expires_at, revoked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (credential_id) DO NOTHING",
            )
            .db()?
            .execute(params![
                token.id.as_bytes().as_slice(),
                token.name,
                token.secret_hash.as_slice(),
                token.created_at,
                token.expires_at,
                token.revoked_at,
            ])
            .db()?;
        Ok(changed == 1)
    }

    fn operator_token(&self, id: &CredentialId) -> Result<Option<OperatorToken>> {
        self.conn
            .prepare_cached(&format!(
                "SELECT {COLUMNS} FROM operator_tokens WHERE credential_id = ?1"
            ))
            .db()?
            .query_row([id.as_bytes().as_slice()], token_of)
            .optional()
            .db()
    }

    fn operator_tokens(&self) -> Result<Vec<OperatorToken>> {
        let mut statement = self
            .conn
            .prepare_cached(&format!(
                "SELECT {COLUMNS} FROM operator_tokens ORDER BY created_at, credential_id"
            ))
            .db()?;
        let rows = statement.query_map([], token_of).db()?;
        rows.collect::<rusqlite::Result<_>>().db()
    }

    fn revoke_operator_token(&mut self, id: &CredentialId, now: i64) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                "UPDATE operator_tokens SET revoked_at = ?2
                 WHERE credential_id = ?1 AND revoked_at IS NULL",
                params![id.as_bytes().as_slice(), now],
            )
            .db()?;
        Ok(changed == 1)
    }

    fn operator_tokens_exist(&self) -> Result<bool> {
        self.conn
            .prepare_cached("SELECT EXISTS (SELECT 1 FROM operator_tokens)")
            .db()?
            .query_row([], |row| row.get::<_, i64>(0))
            .map(|found| found == 1)
            .db()
    }
}
