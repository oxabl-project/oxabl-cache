use oxabl_cache_client::Invite;
use serde_json::json;

use crate::cli::InviteArgs;
use crate::context::Context;
use crate::failure::Failure;
use crate::output::print_json;

/// Prints the invite for the repository's cache; it holds no secret.
pub fn run(context: &Context, args: InviteArgs) -> Result<(), Failure> {
    let config = context.session_config()?;
    let invite =
        Invite::new(&config, &args.name, &args.issuer, &args.client_id).map_err(Failure::usage)?;
    if context.json {
        print_json(&json!({"invite": invite.to_text()}));
    } else {
        println!("{}", invite.to_text());
    }
    Ok(())
}
