use oxabl_cache_client::{DeviceLogin, logout};
use serde_json::json;

use crate::cli::LoginArgs;
use crate::context::Context;
use crate::failure::Failure;
use crate::output::print_json;

fn login_failure(error: oxabl_cache_client::LoginError) -> Failure {
    match error {
        oxabl_cache_client::LoginError::Config(_) => Failure::usage(error),
        other => Failure::refused(other),
    }
}

/// Shows the person a code, waits for the approval and stores the session.
pub async fn run(context: &Context, args: LoginArgs) -> Result<(), Failure> {
    let config = context.session_config()?;
    let login = DeviceLogin::new(&config, &args.issuer, &args.client_id).map_err(login_failure)?;
    let challenge = login.begin().await.map_err(login_failure)?;
    eprintln!(
        "Open {} and enter the code {}",
        challenge.verification_uri, challenge.user_code
    );
    if let Some(complete) = &challenge.verification_uri_complete {
        eprintln!("or open {complete}");
    }
    let session = login.finish(&challenge).await.map_err(login_failure)?;
    if context.json {
        print_json(&json!({"logged_in": true, "expires_at": session.expires_at}));
    } else {
        println!(
            "logged in; the session expires at unix time {}",
            session.expires_at
        );
    }
    Ok(())
}

/// Revokes the stored session and deletes its file.
pub async fn run_logout(context: &Context) -> Result<(), Failure> {
    let config = context.session_config()?;
    let outcome = logout(&config).await.map_err(login_failure)?;
    if context.json {
        print_json(&json!({"removed": outcome.removed, "revoked": outcome.revoked}));
    } else if !outcome.removed {
        println!("no stored session");
    } else if outcome.revoked {
        println!("logged out; the session is revoked");
    } else {
        println!("the stored session is deleted");
    }
    if outcome.removed && !outcome.revoked {
        return Err(Failure::refused(
            "the server could not confirm the revocation: the session stays valid there until it expires",
        ));
    }
    Ok(())
}
