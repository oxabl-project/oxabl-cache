//! User rows.

use crate::db::{DbResult, RowExt};
use crate::tx::SqliteTx;
use oxabl_cache_core::Result;
use oxabl_cache_core::ids::{PrincipalId, TenantId};
use oxabl_cache_core::ports::metadata::{User, UsersTx};
use rusqlite::{OptionalExtension, params};

impl UsersTx for SqliteTx<'_> {
    fn insert_user(&mut self, user: &User) -> Result<bool> {
        let changed = self
            .conn
            .prepare_cached(
                "INSERT INTO users
                   (principal_id, tenant_id, issuer, subject, label, created_at, disabled_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT DO NOTHING",
            )
            .db()?
            .execute(params![
                user.id.as_bytes().as_slice(),
                user.tenant.as_bytes().as_slice(),
                user.issuer,
                user.subject,
                user.label,
                user.created_at,
                user.disabled_at,
            ])
            .db()?;
        Ok(changed == 1)
    }

    fn user(&self, id: &PrincipalId) -> Result<Option<User>> {
        self.conn
            .prepare_cached(
                "SELECT principal_id, tenant_id, issuer, subject, label, created_at, disabled_at
                 FROM users WHERE principal_id = ?1",
            )
            .db()?
            .query_row([id.as_bytes().as_slice()], |row| {
                Ok(User {
                    id: PrincipalId::from_bytes(row.blob(0)?),
                    tenant: TenantId::from_bytes(row.blob(1)?),
                    issuer: row.get(2)?,
                    subject: row.get(3)?,
                    label: row.get(4)?,
                    created_at: row.get(5)?,
                    disabled_at: row.get(6)?,
                })
            })
            .optional()
            .db()
    }

    fn disable_user(&mut self, id: &PrincipalId, now: i64) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                "UPDATE users SET disabled_at = ?2 WHERE principal_id = ?1 AND disabled_at IS NULL",
                params![id.as_bytes().as_slice(), now],
            )
            .db()?;
        Ok(changed == 1)
    }
}
