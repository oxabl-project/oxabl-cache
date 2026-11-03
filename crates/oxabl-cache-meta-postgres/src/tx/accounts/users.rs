//! User rows.

use crate::tx::{PgTx, RowExt, args};
use oxabl_cache_core::Result;
use oxabl_cache_core::ids::{PrincipalId, TenantId};
use oxabl_cache_core::ports::metadata::{User, UsersTx};
use tokio_postgres::Row;

pub(in crate::tx) const USER_COLUMNS: &str =
    "principal_id, tenant_id, issuer, subject, label, created_at, disabled_at";

pub(in crate::tx) fn user_of(row: &Row) -> Result<User> {
    Ok(User {
        id: PrincipalId::from_bytes(row.blob(0)?),
        tenant: TenantId::from_bytes(row.blob(1)?),
        issuer: row.text(2)?,
        subject: row.text(3)?,
        label: row.text(4)?,
        created_at: row.int(5)?,
        disabled_at: row.opt_int(6)?,
    })
}

impl UsersTx for PgTx<'_> {
    fn insert_user(&mut self, user: &User) -> Result<bool> {
        let changed = self.execute(
            &format!(
                "INSERT INTO users ({USER_COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT DO NOTHING"
            ),
            args![
                user.id.as_bytes().as_slice(),
                user.tenant.as_bytes().as_slice(),
                user.issuer,
                user.subject,
                user.label,
                user.created_at,
                user.disabled_at,
            ],
        )?;
        Ok(changed == 1)
    }

    fn user(&self, id: &PrincipalId) -> Result<Option<User>> {
        self.query_opt(
            &format!("SELECT {USER_COLUMNS} FROM users WHERE principal_id = $1"),
            args![id.as_bytes().as_slice()],
        )?
        .as_ref()
        .map(user_of)
        .transpose()
    }

    fn disable_user(&mut self, id: &PrincipalId, now: i64) -> Result<bool> {
        let changed = self.execute(
            "UPDATE users SET disabled_at = $2 WHERE principal_id = $1 AND disabled_at IS NULL",
            args![id.as_bytes().as_slice(), now],
        )?;
        Ok(changed == 1)
    }
}
