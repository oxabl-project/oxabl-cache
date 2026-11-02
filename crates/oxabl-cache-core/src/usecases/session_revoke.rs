//! Ending a session.

use crate::domain::credential::CredentialKind;
use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::ports::{Clock, MetadataStore, ObjectStore, PackAssembler};
use crate::usecases::credential_text::parse;

impl<M, S, A, C> Engine<M, S, A, C>
where
    M: MetadataStore,
    S: ObjectStore,
    A: PackAssembler,
    C: Clock,
{
    /// Revokes the session the bearer text names, after authenticating it.
    pub async fn revoke_session(&self, bearer: &str) -> Result<()> {
        self.authenticate(bearer).await?;
        let parsed = parse(bearer).ok_or(Error::Unauthenticated)?;
        if parsed.kind != CredentialKind::Session {
            return Err(Error::bad_request("not a session"));
        }
        let now = self.now();
        self.meta
            .transact(move |tx| {
                tx.revoke_credential(CredentialKind::Session, &parsed.id, now)?;
                Ok(())
            })
            .await
    }
}
