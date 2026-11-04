use oxabl_cache_client::{InitRequest, init, open_identity_file};
use serde_json::json;
use url::Url;

use crate::cli::InitArgs;
use crate::context::Context;
use crate::failure::Failure;
use crate::output::print_json;
use crate::secrets;

/// Publishes the first roster of a cache and prints the `.oxabl/cache.toml` that pins it.
pub async fn run(context: &Context, args: InitArgs) -> Result<(), Failure> {
    let server = Url::parse(&args.server).map_err(|_| Failure::usage("--server is not a URL"))?;
    let path = context.identity_path()?;
    let passphrase = match context.passphrase()? {
        Some(passphrase) => passphrase,
        None => secrets::prompt("Identity passphrase")
            .ok_or_else(|| Failure::usage("no passphrase was entered"))?,
    };
    let identity = tokio::task::spawn_blocking(move || open_identity_file(&path, &passphrase))
        .await
        .map_err(|_| Failure::refused("unlocking the identity failed"))?
        .map_err(Failure::refused)?;
    let mut request = InitRequest::new(server, &args.tenant, &args.cache, &args.label, identity)
        .with_credentials(context.credentials()?);
    if let Some(dir) = context.state_dir() {
        request = request.with_state_dir(dir.to_path_buf());
    }
    if let Some(ca) = context.ca_file() {
        request = request.with_ca_file(ca.to_path_buf());
    }
    let report = init(request).await?;
    if context.json {
        print_json(&json!({
            "cache_toml": report.cache_toml,
            "owner_key": report.owner_key,
            "roster_version": report.roster_version,
            "epoch": report.epoch,
        }));
    } else {
        println!("# .oxabl/cache.toml\n{}", report.cache_toml);
        println!("owner key  {}", report.owner_key);
    }
    Ok(())
}
