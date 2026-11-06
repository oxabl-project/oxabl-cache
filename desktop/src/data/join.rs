use std::sync::Arc;
use std::time::Duration;

use oxabl_cache_client::{Cache, DeviceLogin, Invite, OpenError, logout, signed_in};

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

/// Whether this machine holds an unexpired sign-in for the invite's organisation.
pub fn is_signed_in(paths: &super::paths::Paths, invite: &Invite) -> bool {
    signed_in(&invite.config().with_state_dir(paths.state_dir.clone()))
}

/// The device login: shows the person a code and a page, then stores the session it opens.
pub(crate) async fn sign_in(context: &Arc<Context>, invite: &Invite) {
    let config = config_of(context, invite);
    let login = match DeviceLogin::new(&config, invite.issuer(), invite.client_id()) {
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
    let host = invite.server().host_str().unwrap_or("cache").to_owned();
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
                if matches!(error, OpenError::NoCredentials | OpenError::Authentication) {
                    return context.send(Event::JoinFailed(
                        "Your sign-in has ended. Sign in again to finish joining.".into(),
                    ));
                }
            }
        }
        tokio::time::sleep(MEMBERSHIP_POLL).await;
    }
}

/// Revokes and forgets the organisation's sign-in on this machine.
pub(crate) async fn sign_out(context: &Arc<Context>, invite: &Invite) {
    let config = config_of(context, invite);
    match logout(&config).await {
        Ok(_) => context.send(Event::Notice("Signed out.".into())),
        Err(error) => context.send(Event::Notice(format!("Signing out: {error}"))),
    }
}
