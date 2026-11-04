use oxabl_cache_client::{Owner, open_identity_file};

use super::report::{Next, print};
use crate::cli::OwnerCommand;
use crate::context::Context;
use crate::failure::Failure;
use crate::secrets;

pub async fn run(context: &Context, command: OwnerCommand) -> Result<(), Failure> {
    let owner = Owner::open(context.config()?).await?;
    let next = Next::OwnerKeys {
        cache: owner.cache_id(),
    };
    match command {
        OwnerCommand::Add {
            descriptor,
            label,
            cosign_identity_file,
            cosign_passphrase_file,
        } => {
            let passphrase = match cosign_passphrase_file {
                Some(path) => secrets::read_passphrase_file(&path)?,
                None => secrets::prompt("Cosigning identity passphrase")
                    .ok_or_else(|| Failure::usage("no passphrase was entered"))?,
            };
            let cosigner = tokio::task::spawn_blocking(move || {
                open_identity_file(&cosign_identity_file, &passphrase)
            })
            .await
            .map_err(|_| Failure::refused("unlocking the identity failed"))?
            .map_err(Failure::refused)?;
            let published = owner.add_owner(&descriptor, &label, &cosigner).await?;
            print(context.json, &published, &next)
        }
        OwnerCommand::Remove { owner: who, purge } => {
            let published = owner.remove_owner(&who, purge).await?;
            print(context.json, &published, &next)
        }
    }
}
