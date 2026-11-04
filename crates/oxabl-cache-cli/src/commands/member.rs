use oxabl_cache_client::{Owner, Role};

use super::report::{Next, print};
use crate::cli::{MemberCommand, RoleArg};
use crate::context::Context;
use crate::failure::Failure;

pub async fn run(context: &Context, command: MemberCommand) -> Result<(), Failure> {
    let owner = Owner::open(context.config()?).await?;
    match command {
        MemberCommand::Add {
            descriptor,
            label,
            role,
        } => {
            let role = match role {
                RoleArg::Ci => Role::Ci,
                RoleArg::Developer => Role::Developer,
                RoleArg::Reader => Role::Reader,
            };
            let published = owner.add_member(&descriptor, &label, role).await?;
            print(context.json, &published, &Next::Nothing)
        }
        MemberCommand::Remove { label, purge } => {
            let published = owner.remove_member(&label, purge).await?;
            print(context.json, &published, &Next::Departure)
        }
    }
}

pub async fn rotate(context: &Context, purge: bool) -> Result<(), Failure> {
    let owner = Owner::open(context.config()?).await?;
    let published = owner.rotate(purge).await?;
    print(context.json, &published, &Next::Departure)
}
