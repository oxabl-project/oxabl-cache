use std::path::{Path, PathBuf};

use oxabl_cache_client::{
    Config, CreatedToken, Credentials, Identity, IdentityError, IdentitySource, NewToken, Owner,
    OwnerError, Published, Role, SecretString, TenantAdmin, me, open_identity_file, role_name,
    signing_key_text, write_identity_secret,
};
use serde_json::json;

use super::token::{create_into, open_admin, scope_list};
use crate::cli::{CiSystemRole, OnboardCiArgs};
use crate::context::Context;
use crate::failure::Failure;
use crate::output::{print_fields, print_json};
use crate::secrets;

const IDENTITY_FILE: &str = "OXABL_CACHE_IDENTITY";
const TOKEN_FILE: &str = "OXABL_CACHE_TOKEN";

/// The roster role and the token scopes on the cache that the server's rules tie to it: a `ci`
/// member signs trusted entries, which needs `write:trusted`; a `reader` only fetches.
fn rules(role: CiSystemRole) -> (Role, &'static [&'static str], &'static str) {
    match role {
        CiSystemRole::Ci => (Role::Ci, &["read", "write:trusted"], "ci"),
        CiSystemRole::Reader => (Role::Reader, &["read"], "read-only"),
    }
}

/// Unlocks the owner's identity file with the passphrase it will be used with, so a wrong
/// passphrase is named as such. Returns the passphrase when it came from the terminal.
async fn unlock_owner(context: &Context) -> Result<Option<SecretString>, Failure> {
    let IdentitySource::File(path) = context.identity_source()? else {
        return Ok(None);
    };
    let passphrase = match context.passphrase(Some(&path))? {
        Some(passphrase) => passphrase,
        None => secrets::prompt("Identity passphrase")
            .ok_or_else(|| Failure::usage("no passphrase was entered"))?,
    };
    let (file, secret) = (path.clone(), passphrase.clone());
    tokio::task::spawn_blocking(move || open_identity_file(&file, &secret))
        .await
        .map_err(|_| Failure::refused("unlocking the identity failed"))?
        .map_err(|error| match error {
            IdentityError::Unlock => Failure::refused(format!(
                "the passphrase does not unlock the owner identity {}",
                path.display()
            )),
            other => Failure::refused(format!("{}: {other}", path.display())),
        })?;
    Ok(Some(passphrase))
}

/// Takes back what was done before the roster changed: revokes the token and removes the files.
struct Undo<'a> {
    admin: &'a TenantAdmin,
    token_id: String,
    files: Vec<PathBuf>,
}

impl Undo<'_> {
    async fn run(self, cause: impl std::fmt::Display) -> Failure {
        let mut left = Vec::new();
        if let Err(error) = self.admin.revoke_token(&self.token_id).await {
            left.push(format!(
                "the token {} is not revoked ({error}): run `oxabl-cache token revoke {} --admin-token-file <file>`",
                self.token_id, self.token_id
            ));
        }
        for file in &self.files {
            if let Err(error) = std::fs::remove_file(file)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                left.push(format!(
                    "{} is not removed ({error}): delete it",
                    file.display()
                ));
            }
        }
        if left.is_empty() {
            Failure::refused(format!(
                "{cause}; the roster is unchanged, the token {} is revoked and the files are removed",
                self.token_id
            ))
        } else {
            Failure::refused(format!(
                "{cause}; the roster is unchanged, but {}",
                left.join("; ")
            ))
        }
    }
}

/// Whether the error leaves it open that the server stored the roster.
fn may_have_published(error: &OwnerError) -> bool {
    match error {
        OwnerError::Unreachable | OwnerError::Malformed => true,
        OwnerError::Refused { status, .. } => *status >= 500,
        _ => false,
    }
}

/// Creates the identity, the token and the roster entry of a CI system. The roster changes
/// last: every earlier step is undone with the token and the files, and the roster never needs
/// a rollback, which would start a new epoch.
pub async fn ci(context: &Context, args: OnboardCiArgs) -> Result<(), Failure> {
    let (role, scopes, mode) = rules(args.role);
    let identity_path = args.out.join(IDENTITY_FILE);
    let token_path = args.out.join(TOKEN_FILE);
    if args.out.exists() && !args.out.is_dir() {
        return Err(Failure::usage(format!(
            "{} is not a directory",
            args.out.display()
        )));
    }
    secrets::ensure_absent(&identity_path)?;
    secrets::ensure_absent(&token_path)?;

    let mut config = context.config()?;
    if let Some(passphrase) = unlock_owner(context).await? {
        config = config.with_passphrase(move || Some(passphrase.clone()));
    }
    let owner = Owner::open(config.clone()).await?;
    let cache = owner.cache_id();
    let owner_scopes = me(&config)
        .await
        .map_err(|e| Failure::refused(format!("the owner's token: {e}")))?
        .caches
        .into_iter()
        .find(|c| c.id == cache)
        .map(|c| c.scopes)
        .unwrap_or_default();
    if !owner_scopes.iter().any(|s| s == "cache:admin") {
        return Err(Failure::refused(format!(
            "the owner's token has no cache:admin on {cache}: pass --token-file with a token that has it"
        )));
    }
    owner.check_new_member(&args.name, role)?;
    let (admin, _) = open_admin(context, &args.admin_token_file).await?;
    if !admin.has_cache(&cache) {
        return Err(Failure::refused(format!("the tenant has no cache {cache}")));
    }

    let spec = NewToken {
        name: args.name.clone(),
        scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
        caches: vec![cache.clone()],
        expires_in_days: args.expires_in_days,
    };
    let created = create_into(&admin, &spec, &token_path).await?;
    let mut undo = Undo {
        admin: &admin,
        token_id: created.id.clone(),
        files: vec![token_path.clone()],
    };

    let granted = me(&config
        .clone()
        .with_credentials(Credentials::Token(created.token.clone())))
    .await;
    let granted = match granted {
        Ok(me) => me,
        Err(error) => {
            return Err(undo
                .run(format!("the new token does not work: {error}"))
                .await);
        }
    };
    let on_cache: Vec<String> = granted
        .caches
        .iter()
        .filter(|c| c.id == cache)
        .flat_map(|c| c.scopes.clone())
        .collect();
    let mut wanted = spec.scopes.clone();
    wanted.sort();
    let mut have = on_cache.clone();
    have.sort();
    if have != wanted || granted.tenant_admin {
        return Err(undo
            .run(format!(
                "the server gave the token other scopes than the role {} needs (wanted {}; got {}{})",
                role_name(role),
                scope_list(&wanted),
                scope_list(&have),
                if granted.tenant_admin { ", and tenant:admin" } else { "" }
            ))
            .await);
    }

    let identity =
        Identity::generate().map_err(|_| Failure::refused("the random generator failed"));
    let identity = match identity {
        Ok(identity) => identity,
        Err(failure) => return Err(undo.run(failure).await),
    };
    if let Err(error) = write_identity_secret(&identity_path, &identity) {
        return Err(undo
            .run(format!("{}: {error}", identity_path.display()))
            .await);
    }
    undo.files.push(identity_path.clone());
    let descriptor = match identity.descriptor_string() {
        Ok(descriptor) => descriptor,
        Err(error) => return Err(undo.run(error).await),
    };

    let published = match owner.add_member(&descriptor, &args.name, role).await {
        Ok(published) => published,
        Err(error) if may_have_published(&error) => {
            return Err(settle(&config, &args.name, &identity, undo, error).await);
        }
        Err(error) => return Err(undo.run(error).await),
    };
    print_done(
        context,
        &Done {
            label: &args.name,
            role: role_name(role),
            mode,
            scopes: &spec.scopes,
            token: &created,
            published: &published,
            identity_path: &identity_path,
            token_path: &token_path,
        },
    );
    Ok(())
}

/// The roster change got no clear answer: reads the roster to learn whether it holds the new
/// member, and undoes the earlier steps only when it does not.
async fn settle(
    config: &Config,
    label: &str,
    identity: &Identity,
    undo: Undo<'_>,
    error: OwnerError,
) -> Failure {
    let key = signing_key_text(identity);
    let listed = Owner::open(config.clone()).await.ok().map(|owner| {
        owner
            .roster()
            .members
            .iter()
            .any(|m| m.label == label && m.signing_keys.contains(&key))
    });
    match listed {
        Some(false) => undo.run(error).await,
        Some(true) => Failure::refused(format!(
            "{error}; the roster holds the member {label} nevertheless, so the files in {} are valid: store them, or remove the member with `oxabl-cache member remove {label}`",
            undo.files[0].parent().unwrap_or(Path::new(".")).display()
        )),
        None => Failure::refused(format!(
            "{error}; it is not known whether the roster holds the member {label}: run `oxabl-cache status`. If it does, the files in {} are valid; if not, run `oxabl-cache token revoke {} --admin-token-file <file>` and delete the files",
            undo.files[0].parent().unwrap_or(Path::new(".")).display(),
            undo.token_id
        )),
    }
}

/// What the command did, for the report.
struct Done<'a> {
    label: &'a str,
    role: &'a str,
    mode: &'a str,
    scopes: &'a [String],
    token: &'a CreatedToken,
    published: &'a Published,
    identity_path: &'a Path,
    token_path: &'a Path,
}

fn print_done(context: &Context, done: &Done) {
    let check = format!("oxabl-cache --mode {} status", done.mode);
    let (label, role) = (done.label, done.role);
    let (version, epoch) = (done.published.roster_version, done.published.epoch);
    if context.json {
        print_json(&json!({
            "member": {"label": label, "role": role, "roster_version": version, "epoch": epoch},
            "token": {"id": done.token.id, "scopes": done.scopes, "expires_at": done.token.expires_at},
            "secrets": {
                IDENTITY_FILE: done.identity_path,
                TOKEN_FILE: done.token_path,
            },
            "check": check,
        }));
        return;
    }
    print_fields(&[
        (
            "member",
            format!("{label} ({role}), roster v{version}, epoch {epoch}"),
        ),
        (
            "token",
            format!("{}, {}", done.token.id, scope_list(done.scopes)),
        ),
        ("expires at", format!("unix time {}", done.token.expires_at)),
    ]);
    println!(
        "\nStore these files as secrets of the CI system, under these names, then delete them:\n  {IDENTITY_FILE:<22} {}\n  {TOKEN_FILE:<22} {}",
        done.identity_path.display(),
        done.token_path.display()
    );
    println!(
        "\nCheck, in a checkout of the repository with both secrets in the environment:\n  {check}"
    );
}
