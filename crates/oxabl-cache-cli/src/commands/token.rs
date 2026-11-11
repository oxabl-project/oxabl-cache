use std::path::Path;

use oxabl_cache_client::{Config, CreatedToken, NewToken, TenantAdmin, TenantError, TokenInfo};
use serde_json::json;

use crate::cli::TokenCommand;
use crate::context::Context;
use crate::failure::Failure;
use crate::output::{print_fields, print_json};
use crate::secrets;

/// Checks the admin token against the repository's tenant and cache.
pub async fn open_admin(
    context: &Context,
    admin_token_file: &Path,
) -> Result<(TenantAdmin, Config), Failure> {
    let token = secrets::read_token_file(admin_token_file)?;
    let config = context.session_config()?;
    let admin = TenantAdmin::open(&config, token)
        .await
        .map_err(|error| match error {
            TenantError::Authentication
            | TenantError::NotTenantAdmin
            | TenantError::WrongTenant => Failure::refused(format!(
                "admin token {}: {error}",
                admin_token_file.display()
            )),
            other => other.into(),
        })?;
    Ok((admin, config))
}

/// The scopes in the spelling of the tokens' API, in a stable order.
pub fn scope_list(scopes: &[String]) -> String {
    let mut sorted = scopes.to_vec();
    sorted.sort();
    sorted.join(", ")
}

/// Creates the token and stores its secret in `out`. When the file cannot be written the token
/// is revoked, so no token exists that nobody holds.
pub async fn create_into(
    admin: &TenantAdmin,
    spec: &NewToken,
    out: &Path,
) -> Result<CreatedToken, Failure> {
    secrets::ensure_absent(out)?;
    let created = admin.create_token(spec).await?;
    if let Err(failure) = secrets::write_new_secret_file(out, &created.token) {
        return Err(match admin.revoke_token(&created.id).await {
            Ok(()) => Failure::refused(format!(
                "{failure}; the token {} was revoked again",
                created.id
            )),
            Err(error) => Failure::refused(format!(
                "{failure}; the token {} still exists and nobody holds it ({error}): revoke it with `oxabl-cache token revoke {} --admin-token-file <file>`",
                created.id, created.id
            )),
        });
    }
    Ok(created)
}

pub async fn run(context: &Context, command: TokenCommand) -> Result<(), Failure> {
    match command {
        TokenCommand::Create {
            admin_token_file,
            scopes,
            name,
            out,
            expires_in_days,
        } => {
            secrets::ensure_absent(&out)?;
            let (admin, config) = open_admin(context, &admin_token_file).await?;
            let caches = if scopes.iter().all(|s| s == "tenant:admin") {
                Vec::new()
            } else {
                if !admin.has_cache(&config.cache()) {
                    return Err(Failure::refused(format!(
                        "the tenant has no cache {}",
                        config.cache()
                    )));
                }
                vec![config.cache()]
            };
            let spec = NewToken {
                name,
                scopes,
                caches,
                expires_in_days,
            };
            let created = create_into(&admin, &spec, &out).await?;
            if context.json {
                print_json(&json!({
                    "id": created.id,
                    "name": spec.name,
                    "scopes": spec.scopes,
                    "caches": spec.caches,
                    "expires_at": created.expires_at,
                    "token_file": out,
                }));
            } else {
                print_fields(&[
                    ("id", created.id),
                    ("name", spec.name),
                    ("scopes", scope_list(&spec.scopes)),
                    ("expires at", format!("unix time {}", created.expires_at)),
                    ("token file", out.display().to_string()),
                ]);
            }
            Ok(())
        }
        TokenCommand::List { admin_token_file } => {
            let (admin, _) = open_admin(context, &admin_token_file).await?;
            let tokens = admin.list_tokens().await?;
            if context.json {
                print_json(&json!({"tokens": tokens.iter().map(token_json).collect::<Vec<_>>()}));
            } else {
                for token in &tokens {
                    print_token(token);
                }
            }
            Ok(())
        }
        TokenCommand::Revoke {
            id,
            admin_token_file,
        } => {
            let (admin, _) = open_admin(context, &admin_token_file).await?;
            admin.revoke_token(&id).await?;
            if context.json {
                print_json(&json!({"revoked": id}));
            } else {
                println!("revoked {id}");
            }
            Ok(())
        }
    }
}

fn token_json(token: &TokenInfo) -> serde_json::Value {
    json!({
        "id": token.id,
        "name": token.name,
        "principal": token.principal,
        "created_at": token.created_at,
        "expires_at": token.expires_at,
        "revoked_at": token.revoked_at,
        "grants": token.grants.iter().map(|g| json!({"cache": g.cache, "scopes": g.scopes})).collect::<Vec<_>>(),
    })
}

fn print_token(token: &TokenInfo) {
    let grants: Vec<String> = token
        .grants
        .iter()
        .map(|g| {
            format!(
                "{}: {}",
                g.cache.as_deref().unwrap_or("tenant"),
                scope_list(&g.scopes)
            )
        })
        .collect();
    let state = match token.revoked_at {
        Some(at) => format!("revoked at unix time {at}"),
        None => format!("expires at unix time {}", token.expires_at),
    };
    println!("{:<28} {:<24} {state}", token.id, token.name);
    for grant in grants {
        println!("  {grant}");
    }
}
