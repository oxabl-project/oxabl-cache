//! `tenant ...`.

use crate::cli::TenantCommand;
use crate::client::Admin;
use crate::error::Failure;
use crate::ids::path_id;
use crate::output::{print_fields, print_json, print_table};
use crate::units::parse_size;
use reqwest::Method;
use serde_json::{Map, Value, json};

const FIELDS: &[&str] = &[
    "id",
    "name",
    "status",
    "hard_bytes",
    "egress_bytes",
    "rate",
    "burst",
    "created_at",
];

fn list(admin: &Admin) -> Result<Vec<Value>, Failure> {
    let body = admin.json(Method::GET, "/admin/v1/tenants", None)?;
    Ok(body["tenants"].as_array().cloned().unwrap_or_default())
}

fn find(admin: &Admin, tenant: &str) -> Result<Value, Failure> {
    let tenants = list(admin)?;
    tenants
        .into_iter()
        .find(|row| row["id"] == tenant || row["name"] == tenant)
        .ok_or_else(|| Failure::refused(404, "not_found", format!("no tenant '{tenant}'")))
}

/// The id of a tenant given by id or by name.
pub fn resolve(admin: &Admin, tenant: &str) -> Result<String, Failure> {
    let found = find(admin, tenant)?;
    let id = found["id"].as_str().unwrap_or_default();
    path_id(id).map(str::to_owned)
}

fn show_one(json: bool, tenant: &Value) {
    if json {
        print_json(tenant);
    } else {
        print_fields(tenant, FIELDS);
    }
}

fn size_value(flag: &Option<String>) -> Result<Option<Value>, Failure> {
    flag.as_deref()
        .map(|text| parse_size(text).map(|bytes| bytes.map_or(Value::Null, Value::from)))
        .transpose()
}

fn set_status(admin: &Admin, json: bool, tenant: &str, status: &str) -> Result<(), Failure> {
    let id = resolve(admin, tenant)?;
    let updated = admin.json(
        Method::PATCH,
        &format!("/admin/v1/tenants/{id}"),
        Some(&json!({ "status": status })),
    )?;
    show_one(json, &updated);
    Ok(())
}

pub fn run(admin: &Admin, json: bool, command: TenantCommand) -> Result<(), Failure> {
    match command {
        TenantCommand::Create {
            name,
            hard_quota,
            egress,
            rate,
            burst,
        } => {
            let mut body = Map::new();
            body.insert("name".into(), name.into());
            for (key, value) in [
                ("hard_bytes", size_value(&hard_quota)?),
                ("egress_bytes", size_value(&egress)?),
                ("rate", rate.map(Value::from)),
                ("burst", burst.map(Value::from)),
            ] {
                if let Some(value) = value {
                    body.insert(key.into(), value);
                }
            }
            let created = admin.json(
                Method::POST,
                "/admin/v1/tenants",
                Some(&Value::Object(body)),
            )?;
            show_one(json, &created);
        }
        TenantCommand::List => {
            let tenants = list(admin)?;
            if json {
                print_json(&json!({ "tenants": tenants }));
            } else {
                print_table(
                    &tenants,
                    &[
                        "id",
                        "name",
                        "status",
                        "hard_bytes",
                        "egress_bytes",
                        "rate",
                        "burst",
                    ],
                );
            }
        }
        TenantCommand::Show { tenant } => show_one(json, &find(admin, &tenant)?),
        TenantCommand::Suspend { tenant } => set_status(admin, json, &tenant, "suspended")?,
        TenantCommand::Resume { tenant } => set_status(admin, json, &tenant, "active")?,
        TenantCommand::Delete { tenant } => {
            let id = resolve(admin, &tenant)?;
            admin.json(Method::DELETE, &format!("/admin/v1/tenants/{id}"), None)?;
            if json {
                print_json(&json!({ "deleted": id }));
            } else {
                println!("deleted {id}");
            }
        }
        TenantCommand::Quota {
            tenant,
            hard_quota,
            egress,
            rate,
            burst,
        } => {
            let current = find(admin, &tenant)?;
            let id = path_id(current["id"].as_str().unwrap_or_default())?;
            let body = json!({
                "hard_bytes": size_value(&hard_quota)?.unwrap_or_else(|| current["hard_bytes"].clone()),
                "egress_bytes": size_value(&egress)?.unwrap_or_else(|| current["egress_bytes"].clone()),
                "rate": rate.map_or_else(|| current["rate"].clone(), Value::from),
                "burst": burst.map_or_else(|| current["burst"].clone(), Value::from),
            });
            let updated = admin.json(
                Method::PUT,
                &format!("/admin/v1/tenants/{id}/quota"),
                Some(&body),
            )?;
            show_one(json, &updated);
        }
        TenantCommand::BootstrapAdmin {
            tenant,
            issuer,
            subject,
        } => {
            let id = resolve(admin, &tenant)?;
            let created = admin.json(
                Method::POST,
                &format!("/admin/v1/tenants/{id}/admins"),
                Some(&json!({ "issuer": issuer, "subject": subject })),
            )?;
            if json {
                print_json(&created);
            } else {
                print_fields(&created, &["id"]);
            }
        }
    }
    Ok(())
}
