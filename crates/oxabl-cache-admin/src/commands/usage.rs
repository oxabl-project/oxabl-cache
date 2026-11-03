//! `usage export`.

use crate::cli::Format;
use crate::client::Admin;
use crate::error::Failure;
use crate::output::print_json;
use crate::units::check_month;
use reqwest::Method;
use serde_json::Value;

const HEADER: &str = "month,tenant,cache,metric,value";

/// `--json` means JSON; it conflicts with an explicit `--format csv`.
fn choose(format: Option<Format>, json: bool) -> Result<Format, Failure> {
    match (format, json) {
        (Some(Format::Csv), true) => Err(Failure::usage(
            "--json conflicts with --format csv: drop one of them",
        )),
        (Some(format), _) => Ok(format),
        (None, true) => Ok(Format::Json),
        (None, false) => Ok(Format::Csv),
    }
}

pub fn export(
    admin: &Admin,
    month: &str,
    format: Option<Format>,
    json: bool,
) -> Result<(), Failure> {
    let format = choose(format, json)?;
    check_month(month)?;
    match format {
        Format::Csv => {
            let body = admin.text(&format!("/admin/v1/usage?month={month}&format=csv"))?;
            print!("{}", sorted_csv(&body)?);
        }
        Format::Json => {
            let mut body = admin.json(
                Method::GET,
                &format!("/admin/v1/usage?month={month}&format=json"),
                None,
            )?;
            if let Some(rows) = body["usage"].as_array_mut() {
                rows.sort_by_key(key);
            }
            print_json(&body);
        }
    }
    Ok(())
}

fn key(row: &Value) -> (String, String, String) {
    let field = |name: &str| row[name].as_str().unwrap_or_default().to_owned();
    (field("tenant"), field("cache"), field("metric"))
}

fn sorted_csv(body: &str) -> Result<String, Failure> {
    let malformed = || {
        Failure::refused(
            200,
            "bad_response",
            "the usage export is not the documented CSV",
        )
    };
    let mut lines = body.lines();
    if lines.next() != Some(HEADER) {
        return Err(malformed());
    }
    let mut rows: Vec<(&str, &str, &str, &str)> = Vec::new();
    for line in lines {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() != 5 {
            return Err(malformed());
        }
        rows.push((fields[1], fields[2], fields[3], line));
    }
    rows.sort();
    let mut out = format!("{HEADER}\n");
    for (_, _, _, line) in rows {
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}
