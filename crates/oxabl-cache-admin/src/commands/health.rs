//! `health`.

use crate::client::Admin;
use crate::error::Failure;
use crate::output::{print_fields, print_json};
use reqwest::Method;

pub fn run(admin: &Admin, json: bool) -> Result<(), Failure> {
    let health = admin.json(Method::GET, "/admin/v1/health", None)?;
    if json {
        print_json(&health);
    } else {
        print_fields(&health, &["metadata", "store", "gc_leader"]);
    }
    Ok(())
}
