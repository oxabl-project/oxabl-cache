use oxabl_cache_client::{
    Identity, IdentityError, create_identity_file, default_identity_path, signing_key_hex,
    signing_key_text, write_identity_secret,
};
use serde_json::json;

use crate::cli::IdentityNewArgs;
use crate::context::Context;
use crate::failure::Failure;
use crate::output::{print_fields, print_json};
use crate::secrets;

fn refuse(path: &std::path::Path, error: IdentityError) -> Failure {
    match error {
        IdentityError::Exists => Failure::refused(format!(
            "{} exists; an identity is never overwritten",
            path.display()
        )),
        other => Failure::refused(format!("{}: {other}", path.display())),
    }
}

/// Creates the identity file, the CI secret file, or both, and prints the public descriptor.
pub fn new(context: &Context, args: IdentityNewArgs) -> Result<(), Failure> {
    let file = match (&args.file, &args.secret_out) {
        (Some(file), _) => Some(file.clone()),
        (None, Some(_)) => None,
        (None, None) => Some(
            default_identity_path()
                .ok_or_else(|| Failure::usage("no default identity path: pass --file"))?,
        ),
    };
    let identity = match &file {
        Some(path) => {
            let passphrase = match context.passphrase()? {
                Some(passphrase) => passphrase,
                None => secrets::prompt_new("New identity passphrase").ok_or_else(|| {
                    Failure::usage("the passphrase was not entered twice the same")
                })?,
            };
            create_identity_file(path, &passphrase).map_err(|e| refuse(path, e))?
        }
        None => {
            Identity::generate().map_err(|_| Failure::refused("the random generator failed"))?
        }
    };
    if let Some(path) = &args.secret_out {
        write_identity_secret(path, &identity).map_err(|e| refuse(path, e))?;
    }
    let descriptor = identity.descriptor_string().map_err(Failure::refused)?;
    let key = signing_key_text(&identity);
    let key_hex = signing_key_hex(&identity);
    if context.json {
        print_json(&json!({
            "identity_file": file,
            "secret_file": args.secret_out,
            "descriptor": descriptor,
            "signing_key": key,
            "signing_key_hex": key_hex,
        }));
    } else {
        let mut fields = Vec::new();
        if let Some(path) = &file {
            fields.push(("identity file", path.display().to_string()));
        }
        if let Some(path) = &args.secret_out {
            fields.push(("secret file", path.display().to_string()));
        }
        fields.push(("descriptor", descriptor));
        fields.push(("signing key", key));
        fields.push(("signing key (hex)", key_hex));
        print_fields(&fields);
    }
    Ok(())
}
