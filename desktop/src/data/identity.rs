use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use oxabl_cache_client::{
    ExposeSecret, Identity, IdentityError, SecretString, create_identity_file, open_identity_file,
    save_passphrase, stored_passphrase,
};

use super::worker::{Context, Event};

/// Unlocks the identity file off the async threads (scrypt takes about a second).
async fn open(context: &Context, passphrase: SecretString) -> Result<Identity, IdentityError> {
    let path = context.paths.identity_file.clone();
    tokio::task::spawn_blocking(move || open_identity_file(&path, &passphrase))
        .await
        .unwrap_or(Err(IdentityError::Unlock))
}

fn keep(context: &Context, identity: &Identity) -> bool {
    match identity.to_secret_string() {
        Ok(text) => {
            *context.identity.lock().expect("identity lock") = Some(text);
            true
        }
        Err(_) => false,
    }
}

/// At start: unlocks with the keyring's passphrase. Asks the window for one when the identity
/// file exists and the keyring has none, or the one it has no longer fits.
pub(crate) async fn unlock_from_keyring(context: &Context) {
    let path = context.paths.identity_file.clone();
    if !path.is_file() {
        if let Some(identity) = from_environment() {
            keep(context, &identity);
            context.send(Event::Unlocked);
        }
        return;
    }
    let stored = tokio::task::spawn_blocking(move || stored_passphrase(&path))
        .await
        .ok()
        .flatten();
    let Some(passphrase) = stored else {
        context.send(Event::NeedsUnlock);
        return;
    };
    match open(context, passphrase).await {
        Ok(identity) if keep(context, &identity) => context.send(Event::Unlocked),
        _ => context.send(Event::NeedsUnlock),
    }
}

/// Unlocks with a passphrase the person typed, keeping it in the keyring when asked.
pub(crate) async fn unlock(context: &Context, passphrase: SecretString, remember: bool) -> bool {
    match open(context, passphrase.clone()).await {
        Ok(identity) if keep(context, &identity) => {
            if remember {
                let path = context.paths.identity_file.clone();
                let saved =
                    tokio::task::spawn_blocking(move || save_passphrase(&path, &passphrase))
                        .await
                        .map_err(|_| ());
                if !matches!(saved, Ok(Ok(()))) {
                    context.send(Event::Notice(
                        "Unlocked. This computer has no keyring, so you'll be asked again next time."
                            .into(),
                    ));
                }
            }
            context.send(Event::Unlocked);
            true
        }
        _ => {
            context.send(Event::UnlockFailed(
                "That passphrase doesn't unlock this computer's identity.".into(),
            ));
            false
        }
    }
}

/// The identity string in `OXABL_CACHE_IDENTITY`, which the command line and CI use too.
fn from_environment() -> Option<Identity> {
    let text = std::env::var("OXABL_CACHE_IDENTITY")
        .ok()
        .filter(|v| !v.is_empty())?;
    Identity::parse(text.trim()).ok()
}

fn fresh_passphrase() -> Option<SecretString> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).ok()?;
    Some(SecretString::from(URL_SAFE_NO_PAD.encode(bytes)))
}

/// The unlocked identity, made now when this machine has none. A new identity's passphrase is
/// random, kept in the keyring, and returned once so the person can save it elsewhere too.
pub(crate) async fn ensure(context: &Context) -> Result<(Identity, Option<SecretString>), String> {
    if let Some(text) = context.identity() {
        return Identity::parse(text.expose_secret())
            .map(|identity| (identity, None))
            .map_err(|_| "This computer's identity can't be read.".to_owned());
    }
    if context.paths.identity_file.exists() {
        context.send(Event::NeedsUnlock);
        return Err("Unlock this computer's identity first.".into());
    }
    let passphrase = fresh_passphrase().ok_or("The system's random generator failed.")?;
    let path = context.paths.identity_file.clone();
    let made = {
        let passphrase = passphrase.clone();
        tokio::task::spawn_blocking(move || {
            let identity = create_identity_file(&path, &passphrase)?;
            let _ = save_passphrase(&path, &passphrase);
            Ok::<_, IdentityError>(identity)
        })
        .await
        .map_err(|_| "Making an identity failed.".to_owned())?
        .map_err(|e| format!("Making an identity failed: {e}"))?
    };
    keep(context, &made);
    Ok((made, Some(passphrase)))
}

/// Sends the keyring's passphrase to the window, which copies it.
pub(crate) async fn send_passphrase(context: &Context) {
    let path = context.paths.identity_file.clone();
    let stored = tokio::task::spawn_blocking(move || stored_passphrase(&path))
        .await
        .ok()
        .flatten();
    match stored {
        Some(passphrase) => context.send(Event::Passphrase(passphrase)),
        None => context.send(Event::Notice(
            "This computer's keyring holds no passphrase to copy.".into(),
        )),
    }
}
