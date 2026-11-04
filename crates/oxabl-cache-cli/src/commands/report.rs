//! Printing what a roster change published.

use oxabl_cache_client::{Published, Purge};
use serde_json::json;

use crate::failure::Failure;
use crate::output::print_json;

/// What a change asks the repository and the operator to do next.
pub enum Next {
    /// Nothing beyond the roster itself.
    Nothing,
    /// Set `min_roster_version` and revoke the departed principal's credentials.
    Departure,
    /// Update `owner_keys` and register the keys on the server.
    OwnerKeys { cache: String },
}

fn purge_json(purge: Purge) -> serde_json::Value {
    match purge {
        Purge::NotRequested => json!(null),
        Purge::Marked(n) => json!({"entries_marked": n}),
        Purge::Failed => json!({"failed": true}),
    }
}

fn owner_keys_line(keys: &[String]) -> String {
    let quoted: Vec<String> = keys.iter().map(|k| format!("\"{k}\"")).collect();
    format!("owner_keys = [{}]", quoted.join(", "))
}

pub fn print(json: bool, published: &Published, next: &Next) -> Result<(), Failure> {
    if json {
        print_json(&json!({
            "roster_version": published.roster_version,
            "epoch": published.epoch,
            "min_roster_version": published.min_roster_version,
            "owner_keys": published.owner_keys,
            "owner_keys_hex": published.owner_keys_hex,
            "purge": purge_json(published.purge),
        }));
    } else {
        println!(
            "published roster v{} at epoch {}",
            published.roster_version, published.epoch
        );
        match next {
            Next::Nothing => {}
            Next::Departure => {
                println!(
                    "set in .oxabl/cache.toml:\nmin_roster_version = {}",
                    published.min_roster_version
                );
                println!("then revoke the departed member's tokens and sessions on the server");
            }
            Next::OwnerKeys { cache } => {
                println!(
                    "set in .oxabl/cache.toml:\n{}\nmin_roster_version = {}",
                    owner_keys_line(&published.owner_keys),
                    published.min_roster_version
                );
                println!(
                    "and register the keys on the server, as the tenant administrator:\nPUT /v1/tenant/caches/{cache}/owner-keys {}",
                    json!({"owner_keys": published.owner_keys_hex})
                );
            }
        }
        match published.purge {
            Purge::NotRequested => {}
            Purge::Marked(n) => println!("purged {n} entries of older epochs"),
            Purge::Failed => {}
        }
    }
    if published.purge == Purge::Failed {
        return Err(Failure::refused(
            "the roster is published but the server did not purge older epochs: run `rotate --purge` again",
        ));
    }
    Ok(())
}
