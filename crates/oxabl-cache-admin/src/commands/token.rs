//! `token create`.

use super::tenant::resolve;
use crate::client::Admin;
use crate::error::Failure;
use crate::output::{print_fields, print_json};
use crate::units::parse_days;
use reqwest::Method;
use serde_json::{Map, Value};

pub fn create(
    admin: &Admin,
    json: bool,
    tenant: &str,
    scopes: &str,
    expires: Option<&str>,
    name: &str,
) -> Result<(), Failure> {
    if scopes != "tenant:admin" {
        return Err(Failure::usage(
            "the operator mints only tenant:admin tokens; other scopes come from the tenant API",
        ));
    }
    let mut body = Map::new();
    body.insert("name".into(), name.into());
    if let Some(expires) = expires {
        body.insert("expires_in_days".into(), parse_days(expires)?.into());
    }
    let tenant = resolve(admin, tenant)?;
    let created = admin.json(
        Method::POST,
        &format!("/admin/v1/tenants/{tenant}/tokens"),
        Some(&Value::Object(body)),
    )?;
    if json {
        print_json(&created);
    } else {
        print_fields(&created, &["id", "expires_at", "token"]);
    }
    Ok(())
}
