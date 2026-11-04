use oxabl_cache_client::{ArtifactFileRef, ArtifactPath, Cache, StoreOutcome};
use serde_json::json;

use super::keys;
use crate::cli::PutArgs;
use crate::context::Context;
use crate::failure::Failure;
use crate::output::print_json;

fn parse_file(spec: &str) -> Result<ArtifactFileRef, Failure> {
    let (artifact, disk) = spec
        .split_once('=')
        .ok_or_else(|| Failure::usage(format!("--file {spec}: expected <artifact path>=<file>")))?;
    let path =
        ArtifactPath::new(artifact).map_err(|e| Failure::usage(format!("--file {spec}: {e}")))?;
    ArtifactFileRef::from_disk(path, disk.into())
        .map_err(|e| Failure::usage(format!("--file {spec}: {e}")))
}

fn outcome_name(outcome: StoreOutcome) -> String {
    match outcome {
        StoreOutcome::Committed => "committed".to_owned(),
        StoreOutcome::Existed => "existed".to_owned(),
        StoreOutcome::Changed => "changed".to_owned(),
        StoreOutcome::Failed(failure) => format!("failed: {failure:?}").to_lowercase(),
    }
}

/// Stores one key from files on disk, in the namespace the mode writes.
pub async fn run(context: &Context, args: PutArgs) -> Result<(), Failure> {
    let key = keys::parse(&args.key)?;
    let files = args
        .files
        .iter()
        .map(|spec| parse_file(spec))
        .collect::<Result<Vec<_>, _>>()?;
    let cache = Cache::open(context.config()?).await?;
    let uploader = cache.uploader().map_err(Failure::refused)?;
    uploader.put(key, files).await;
    let report = uploader.finish().await;
    let outcome = report
        .outcomes
        .first()
        .map(|(_, outcome)| *outcome)
        .ok_or_else(|| Failure::refused("the upload ended without an outcome"))?;
    let name = outcome_name(outcome);
    if context.json {
        print_json(&json!({
            "key": keys::hex(&key),
            "outcome": name,
            "bytes_uploaded": report.bytes_uploaded,
        }));
    } else {
        println!(
            "{}  {name}  ({} bytes uploaded)",
            keys::hex(&key),
            report.bytes_uploaded
        );
    }
    match outcome {
        StoreOutcome::Committed | StoreOutcome::Existed => Ok(()),
        _ => Err(Failure::refused(format!("not stored: {name}"))),
    }
}
