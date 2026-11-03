//! `cache scrub`.

use crate::client::Admin;
use crate::error::Failure;
use crate::ids::path_id;
use crate::output::{print_fields, print_json};
use reqwest::Method;

pub fn scrub(admin: &Admin, json: bool, cache: &str) -> Result<(), Failure> {
    let path = format!("/admin/v1/caches/{}/scrub", path_id(cache)?);
    let outcome = admin.json(Method::POST, &path, None)?;
    if json {
        print_json(&outcome);
    } else {
        print_fields(&outcome, &["corrupt_segments", "entries_removed"]);
    }
    Ok(())
}
