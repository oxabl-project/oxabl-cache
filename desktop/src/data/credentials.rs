//! Access tokens a person pasted, kept in the keyring per organisation and server. A token in
//! `OXABL_CACHE_TOKEN` takes precedence, as it does for the command line.

use oxabl_cache_client::{
    Credentials, Invite, SecretString, forget_token, save_token, signed_in, stored_token,
};

use super::worker::Context;

fn key_of(invite: &Invite) -> String {
    format!("{} {}", invite.tenant(), invite.server())
}

/// Whether `OXABL_CACHE_TOKEN` is set, which every cache then uses.
pub(crate) fn from_environment() -> bool {
    std::env::var_os("OXABL_CACHE_TOKEN").is_some_and(|v| !v.is_empty())
}

/// The pasted token the invite's cache is opened with, as far as this run has read the keyring.
pub(crate) fn token_of(context: &Context, invite: &Invite) -> Option<SecretString> {
    if from_environment() {
        return None;
    }
    context
        .tokens
        .lock()
        .expect("token lock")
        .get(&key_of(invite))
        .cloned()
        .flatten()
}

/// How the invite's cache is opened: a pasted token, or what the client finds by itself.
pub(crate) fn credentials_of(context: &Context, invite: &Invite) -> Credentials {
    token_of(context, invite).map_or(Credentials::Auto, Credentials::Token)
}

/// Reads the keyring once per organisation for the tokens of `invites`.
pub(crate) async fn load(context: &Context, invites: &[Invite]) {
    for invite in invites {
        let key = key_of(invite);
        if context
            .tokens
            .lock()
            .expect("token lock")
            .contains_key(&key)
        {
            continue;
        }
        let (server, tenant) = (invite.server().clone(), invite.tenant());
        let token = tokio::task::spawn_blocking(move || stored_token(&server, &tenant))
            .await
            .ok()
            .flatten();
        context
            .tokens
            .lock()
            .expect("token lock")
            .insert(key, token);
    }
}

/// Whether this machine has a credential for the invite's organisation: a token in the
/// environment or the keyring, or an unexpired sign-in.
pub(crate) async fn has_any(context: &Context, invite: &Invite) -> bool {
    load(context, std::slice::from_ref(invite)).await;
    from_environment()
        || token_of(context, invite).is_some()
        || signed_in(
            &invite
                .config()
                .with_state_dir(context.paths.state_dir.clone()),
        )
}

/// Keeps `token` for this run, and in the keyring when there is one; false when only for this
/// run.
pub(crate) async fn keep(context: &Context, invite: &Invite, token: SecretString) -> bool {
    context
        .tokens
        .lock()
        .expect("token lock")
        .insert(key_of(invite), Some(token.clone()));
    let (server, tenant) = (invite.server().clone(), invite.tenant());
    matches!(
        tokio::task::spawn_blocking(move || save_token(&server, &tenant, &token)).await,
        Ok(Ok(()))
    )
}

/// Forgets the pasted token, here and in the keyring; false when there was none.
pub(crate) async fn forget(context: &Context, invite: &Invite) -> bool {
    load(context, std::slice::from_ref(invite)).await;
    let had = context
        .tokens
        .lock()
        .expect("token lock")
        .insert(key_of(invite), None)
        .flatten()
        .is_some();
    let (server, tenant) = (invite.server().clone(), invite.tenant());
    let _ = tokio::task::spawn_blocking(move || forget_token(&server, &tenant)).await;
    had
}
