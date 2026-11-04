use oxabl_cache_client::{Cache, Status, role_name};
use serde_json::json;

use crate::context::Context;
use crate::failure::{EXIT_USAGE, Failure};
use crate::output::{print_fields, print_json};

fn mode_name(mode: oxabl_cache_client::Mode) -> &'static str {
    match mode {
        oxabl_cache_client::Mode::Ci => "ci",
        oxabl_cache_client::Mode::Developer => "developer",
        oxabl_cache_client::Mode::ReadOnly => "read-only",
    }
}

fn print_status(json: bool, status: &Status) {
    let roster = &status.roster;
    let quota = &status.server.quota;
    if json {
        print_json(&json!({
            "on": true,
            "mode": mode_name(status.mode),
            "principal": {
                "label": status.principal.label,
                "role": role_name(status.principal.role),
                "signing_keys": status.principal.signing_keys,
            },
            "roster": {
                "version": roster.version,
                "epoch": roster.epoch,
                "signed_by": roster.signed_by,
                "members": roster.members.iter().map(|m| json!({
                    "label": m.label, "role": role_name(m.role), "signing_keys": m.signing_keys,
                })).collect::<Vec<_>>(),
            },
            "min_roster_version": status.min_roster_version,
            "server": {
                "epoch": status.server.epoch,
                "roster_version": status.server.roster_version,
                "dictionary_version": status.server.dictionary_version,
                "quota": {
                    "hard_bytes": quota.hard_bytes, "live_bytes": quota.live_bytes,
                    "pending_bytes": quota.pending_bytes, "stored_bytes": quota.stored_bytes,
                    "reclaimable_bytes": quota.reclaimable_bytes,
                },
            },
        }));
        return;
    }
    print_fields(&[
        ("cache", "on".to_owned()),
        ("mode", mode_name(status.mode).to_owned()),
        (
            "principal",
            format!(
                "{} ({})",
                status.principal.label,
                role_name(status.principal.role)
            ),
        ),
        (
            "roster",
            format!(
                "v{}, signed by {}",
                roster.version,
                roster.signed_by.join(", ")
            ),
        ),
        ("epoch", roster.epoch.to_string()),
        ("min_roster_version", status.min_roster_version.to_string()),
        (
            "dictionary",
            format!("v{}", status.server.dictionary_version),
        ),
        (
            "quota",
            format!(
                "{} live of {} hard, {} pending, {} stored ({} reclaimable)",
                quota.live_bytes,
                quota.hard_bytes,
                quota.pending_bytes,
                quota.stored_bytes,
                quota.reclaimable_bytes
            ),
        ),
    ]);
    println!("members");
    for member in &roster.members {
        println!("  {:<24} {}", member.label, role_name(member.role));
    }
}

/// Opens the cache and reports it, or reports why it is off.
pub async fn run(context: &Context) -> Result<(), Failure> {
    let off = |reason: String, code: u8| {
        if context.json {
            print_json(&json!({"on": false, "reason": reason}));
        } else {
            println!("cache  off\nreason  {reason}");
        }
        if code == EXIT_USAGE {
            Failure::usage(reason)
        } else {
            Failure::refused(reason)
        }
    };
    let config = context.config()?;
    let cache = match Cache::open(config).await {
        Ok(cache) => cache,
        Err(error) => {
            let code = Failure::from(error.clone()).exit_code();
            return Err(off(error.to_string(), code));
        }
    };
    match cache.status().await {
        Ok(status) => {
            print_status(context.json, &status);
            Ok(())
        }
        Err(error) => Err(off(error.to_string(), 1)),
    }
}
