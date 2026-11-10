use std::sync::Arc;
use std::time::Duration;

use oxabl_cache_client::{
    Cache, Credentials, DeviceLogin, Invite, OpenError, SecretString, logout, me, signed_in,
};

use super::credentials;
use super::identity;
use super::model::Standing;
use super::refresh::{config_of, standing_of};
use super::worker::{Context, Event};

const MEMBERSHIP_POLL: Duration = Duration::from_secs(3);

/// The label an owner sees for this machine: the user and the host name.
pub fn machine_label() -> String {
    let user = std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "me".into());
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|h| h.trim().to_owned())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "desktop".into());
    format!("{user}@{host}")
}

fn host_of(invite: &Invite) -> String {
    invite.server().host_str().unwrap_or("cache").to_owned()
}

/// Tells the window what joining still needs: nothing when this machine has a credential for the
/// organisation, else a sign-in where the invite names one, else an access token.
pub(crate) async fn prepare(context: &Arc<Context>, invite: &Invite) {
    if credentials::has_any(context, invite).await {
        context.send(Event::SignedIn);
    } else if invite.sign_in().is_some() {
        context.send(Event::NeedsSignIn);
    } else {
        context.send(Event::NeedsToken(None));
    }
}

/// Checks a pasted token against the server and keeps it. A token the server refuses, or one
/// without access to the invite's cache, is not kept.
pub(crate) async fn use_token(context: &Arc<Context>, invite: &Invite, token: SecretString) {
    let host = host_of(invite);
    let config = config_of(context, invite).with_credentials(Credentials::Token(token.clone()));
    let refused = match me(&config).await {
        Ok(seen) => (!seen
            .caches
            .iter()
            .any(|c| c.id == invite.cache() && !c.scopes.is_empty()))
        .then(|| format!("That token has no access to {}.", invite.name())),
        Err(OpenError::NoCredentials | OpenError::Authentication) => {
            Some(format!("{host} doesn't accept that token."))
        }
        Err(OpenError::Unreachable) => Some(format!("Can't reach {host}. Try again.")),
        // A server without `/v1/me`; waiting for an owner opens the cache with the token, and
        // a refused token comes back here.
        Err(OpenError::Server(404)) => None,
        Err(error) => Some(format!("Couldn't check the token: {error}")),
    };
    if let Some(why) = refused {
        return context.send(Event::TokenRefused(why));
    }
    if !credentials::keep(context, invite, token).await {
        context.send(Event::Notice(
            "This computer has no keyring, so you'll paste the token again next time.".into(),
        ));
    }
    context.send(Event::SignedIn);
}

/// The device login: shows the person a code and a page, then stores the session it opens.
pub(crate) async fn sign_in(context: &Arc<Context>, invite: &Invite) {
    let Some((issuer, client_id)) = invite.sign_in() else {
        return context.send(Event::NeedsToken(None));
    };
    let config = config_of(context, invite);
    let login = match DeviceLogin::new(&config, issuer, client_id) {
        Ok(login) => login,
        Err(error) => return context.send(Event::SignInFailed(error.to_string())),
    };
    let challenge = match login.begin().await {
        Ok(challenge) => challenge,
        Err(error) => {
            return context.send(Event::SignInFailed(format!(
                "Couldn't start signing in: {error}"
            )));
        }
    };
    context.send(Event::SignInCode {
        url: challenge
            .verification_uri_complete
            .clone()
            .unwrap_or_else(|| challenge.verification_uri.clone()),
        code: challenge.user_code.clone(),
    });
    match login.finish(&challenge).await {
        Ok(_) => context.send(Event::SignedIn),
        Err(error) => context.send(Event::SignInFailed(format!("Signing in failed: {error}"))),
    }
}

/// Makes sure this machine has an identity, tells the window what to send an owner, then waits
/// until the roster lists the identity.
pub(crate) async fn join(context: &Arc<Context>, invite: &Invite) {
    let (identity, new_passphrase) = match identity::ensure(context).await {
        Ok(made) => made,
        Err(why) => return context.send(Event::JoinFailed(why)),
    };
    let descriptor = match identity.descriptor_string() {
        Ok(descriptor) => descriptor,
        Err(_) => {
            return context.send(Event::JoinFailed(
                "This computer's identity can't be read.".into(),
            ));
        }
    };
    context.send(Event::JoinRequest {
        command: format!(
            "oxabl-cache member add {descriptor} --label {} --role developer",
            machine_label()
        ),
        new_passphrase,
    });
    let host = host_of(invite);
    loop {
        match Cache::open(config_of(context, invite)).await {
            Ok(_) => {
                return context.send(Event::Joined {
                    cache: invite.cache(),
                });
            }
            Err(OpenError::IdentityNotInRoster | OpenError::NoRoster | OpenError::Unreachable) => {}
            Err(error) => {
                if let Standing::Trouble(why) = standing_of(&error, &host) {
                    return context.send(Event::JoinFailed(why));
                }
                let refused = matches!(error, OpenError::NoCredentials | OpenError::Authentication);
                if refused
                    && (invite.sign_in().is_none()
                        || credentials::token_of(context, invite).is_some())
                {
                    return context.send(Event::NeedsToken(Some(format!(
                        "{host} doesn't accept this computer's access token. Paste a new one."
                    ))));
                }
                if refused {
                    return context.send(Event::JoinFailed(
                        "Your sign-in has ended. Sign in again to finish joining.".into(),
                    ));
                }
            }
        }
        tokio::time::sleep(MEMBERSHIP_POLL).await;
    }
}

/// Forgets the organisation's access token on this machine, or revokes and forgets its sign-in.
/// A token is the administrator's to revoke, so it is only forgotten here.
pub(crate) async fn sign_out(context: &Arc<Context>, invite: &Invite) {
    if credentials::forget(context, invite).await {
        return context.send(Event::Notice("Forgot the access token.".into()));
    }
    let config = config_of(context, invite);
    if !signed_in(&config) {
        return context.send(Event::Notice("Not signed in.".into()));
    }
    match logout(&config).await {
        Ok(_) => context.send(Event::Notice("Signed out.".into())),
        Err(error) => context.send(Event::Notice(format!("Signing out: {error}"))),
    }
}
