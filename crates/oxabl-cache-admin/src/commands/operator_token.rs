//! `operator-token ...`.

use crate::cli::OperatorTokenCommand;
use crate::client::Admin;
use crate::error::Failure;
use crate::ids::path_id;
use crate::output::{print_fields, print_json, print_table};
use crate::units::parse_days;
use reqwest::Method;
use serde_json::{Map, Value, json};

pub fn run(admin: &Admin, json: bool, command: OperatorTokenCommand) -> Result<(), Failure> {
    match command {
        OperatorTokenCommand::Create { expires, name } => {
            let mut body = Map::new();
            body.insert("name".into(), name.into());
            if let Some(expires) = expires {
                body.insert("expires_in_days".into(), parse_days(&expires)?.into());
            }
            let created = admin.json(
                Method::POST,
                "/admin/v1/operator-tokens",
                Some(&Value::Object(body)),
            )?;
            if json {
                print_json(&created);
            } else {
                print_fields(&created, &["id", "expires_at", "token"]);
            }
        }
        OperatorTokenCommand::List => {
            let listed = admin.json(Method::GET, "/admin/v1/operator-tokens", None)?;
            let tokens = listed["operator_tokens"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if json {
                print_json(&json!({ "operator_tokens": tokens }));
            } else {
                print_table(
                    &tokens,
                    &["id", "name", "created_at", "expires_at", "revoked_at"],
                );
            }
        }
        OperatorTokenCommand::Revoke { id } => {
            let path = format!("/admin/v1/operator-tokens/{}", path_id(&id)?);
            admin.json(Method::DELETE, &path, None)?;
            if json {
                print_json(&json!({ "revoked": id }));
            } else {
                println!("revoked {id}");
            }
        }
    }
    Ok(())
}
