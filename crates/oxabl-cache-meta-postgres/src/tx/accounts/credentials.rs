//! Tokens and sessions.

use super::grants::{NO_CACHE, grant_of};
use crate::tx::{PgTx, RowExt, args};
use oxabl_cache_core::Result;
use oxabl_cache_core::domain::credential::{Credential, CredentialKind};
use oxabl_cache_core::ids::{CredentialId, PrincipalId, TenantId};
use oxabl_cache_core::ports::metadata::CredentialsTx;

pub(in crate::tx) fn tables(kind: CredentialKind) -> (&'static str, &'static str) {
    match kind {
        CredentialKind::Token => ("tokens", "token_grants"),
        CredentialKind::Session => ("sessions", "session_grants"),
    }
}

impl CredentialsTx for PgTx<'_> {
    fn insert_credential(&mut self, credential: &Credential) -> Result<bool> {
        let (table, grants) = tables(credential.kind);
        let changed = self.execute(
            &format!(
                "INSERT INTO {table}
                   (credential_id, tenant_id, principal_id, name, secret_hash,
                    created_at, expires_at, revoked_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (credential_id) DO NOTHING"
            ),
            args![
                credential.id.as_bytes().as_slice(),
                credential.tenant.as_bytes().as_slice(),
                credential.principal.as_bytes().as_slice(),
                credential.name,
                credential.secret_hash.as_slice(),
                credential.created_at,
                credential.expires_at,
                credential.revoked_at,
            ],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        for grant in &credential.grants {
            let cache = grant.cache.map_or(NO_CACHE, |cache| *cache.as_bytes());
            self.execute(
                &format!(
                    "INSERT INTO {grants} (credential_id, cache_id, scopes) VALUES ($1, $2, $3)
                     ON CONFLICT (credential_id, cache_id) DO UPDATE SET scopes = excluded.scopes"
                ),
                args![
                    credential.id.as_bytes().as_slice(),
                    cache.as_slice(),
                    i64::from(grant.scopes.bits())
                ],
            )?;
        }
        Ok(true)
    }

    fn credential(&self, kind: CredentialKind, id: &CredentialId) -> Result<Option<Credential>> {
        let (table, grants) = tables(kind);
        let found = self.query_opt(
            &format!(
                "SELECT credential_id, tenant_id, principal_id, name, secret_hash,
                        created_at, expires_at, revoked_at
                 FROM {table} WHERE credential_id = $1"
            ),
            args![id.as_bytes().as_slice()],
        )?;
        let Some(row) = found else {
            return Ok(None);
        };
        let mut credential = Credential {
            kind,
            id: CredentialId::from_bytes(row.blob(0)?),
            tenant: TenantId::from_bytes(row.blob(1)?),
            principal: PrincipalId::from_bytes(row.blob(2)?),
            name: row.text(3)?,
            secret_hash: row.blob(4)?,
            created_at: row.int(5)?,
            expires_at: row.int(6)?,
            revoked_at: row.opt_int(7)?,
            grants: Vec::new(),
        };
        for row in self.query(
            &format!(
                "SELECT cache_id, scopes FROM {grants} WHERE credential_id = $1 ORDER BY cache_id"
            ),
            args![id.as_bytes().as_slice()],
        )? {
            credential.grants.push(grant_of(row.blob(0)?, row.int(1)?)?);
        }
        Ok(Some(credential))
    }

    fn revoke_credential(
        &mut self,
        kind: CredentialKind,
        id: &CredentialId,
        now: i64,
    ) -> Result<bool> {
        let (table, _) = tables(kind);
        let changed = self.execute(
            &format!(
                "UPDATE {table} SET revoked_at = $2 WHERE credential_id = $1 AND revoked_at IS NULL"
            ),
            args![id.as_bytes().as_slice(), now],
        )?;
        Ok(changed == 1)
    }

    fn revoke_credentials_of(&mut self, principal: &PrincipalId, now: i64) -> Result<u64> {
        let mut revoked = 0;
        for table in ["tokens", "sessions"] {
            revoked += self.execute(
                &format!(
                    "UPDATE {table} SET revoked_at = $2
                     WHERE principal_id = $1 AND revoked_at IS NULL"
                ),
                args![principal.as_bytes().as_slice(), now],
            )?;
        }
        Ok(revoked)
    }
}
