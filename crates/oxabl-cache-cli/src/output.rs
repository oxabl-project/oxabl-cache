//! Human and JSON rendering of results.

use serde_json::Value;

pub fn print_json(value: &Value) {
    println!("{value}");
}

/// A `key  value` line per field.
pub fn print_fields(fields: &[(&str, String)]) {
    let width = fields.iter().map(|(key, _)| key.len()).max().unwrap_or(0);
    for (key, value) in fields {
        println!("{key:<width$}  {value}");
    }
}
