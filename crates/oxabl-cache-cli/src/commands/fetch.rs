use oxabl_cache_client::{
    ArtifactPath, Cache, FetchOptions, FetchOutcome, FetchRequest, MissReason, Tier,
};
use serde_json::json;

use super::keys;
use crate::cli::FetchArgs;
use crate::context::Context;
use crate::failure::Failure;
use crate::output::{print_fields, print_json};

pub fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Trusted => "trusted",
        Tier::Personal => "personal",
    }
}

pub fn miss_name(reason: MissReason) -> String {
    match reason {
        MissReason::Install(kind) => format!("install: {kind}"),
        other => format!("{other:?}").to_lowercase(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Fetches one key into a stage directory and reports what was installed.
pub async fn run(context: &Context, args: FetchArgs) -> Result<(), Failure> {
    let key = keys::parse(&args.key)?;
    let expect = if args.expect.is_empty() {
        None
    } else {
        Some(
            args.expect
                .iter()
                .map(|p| {
                    ArtifactPath::new(p).map_err(|e| Failure::usage(format!("--expect {p}: {e}")))
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
    };
    let cache = Cache::open(context.config()?).await?;
    let mut fetch = cache.fetch(
        vec![FetchRequest { key, expect }],
        FetchOptions::new(args.stage),
    );
    let outcome = fetch
        .next()
        .await
        .ok_or_else(|| Failure::refused("the fetch ended without an outcome"))?;
    match outcome {
        FetchOutcome::Installed {
            files, provenance, ..
        } => {
            if context.json {
                print_json(&json!({
                    "key": keys::hex(&key),
                    "installed": true,
                    "tier": tier_name(provenance.tier),
                    "signer": hex(&provenance.signer),
                    "epoch": provenance.epoch,
                    "roster_version": provenance.roster_version,
                    "files": files.iter().map(|f| json!({"path": f.path.as_str(), "size": f.size})).collect::<Vec<_>>(),
                }));
            } else {
                print_fields(&[
                    ("key", keys::hex(&key)),
                    ("tier", tier_name(provenance.tier).to_owned()),
                    ("signer", hex(&provenance.signer)),
                    ("epoch", provenance.epoch.to_string()),
                ]);
                for file in &files {
                    println!("  {} ({} bytes)", file.path, file.size);
                }
            }
            Ok(())
        }
        FetchOutcome::Miss { reason, .. } => {
            if context.json {
                print_json(
                    &json!({"key": keys::hex(&key), "installed": false, "miss": miss_name(reason)}),
                );
            }
            Err(Failure::refused(format!("miss: {}", miss_name(reason))))
        }
    }
}
