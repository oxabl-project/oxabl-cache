//! Human and JSON rendering of results.

use serde_json::Value;

pub fn print_json(value: &Value) {
    println!("{value}");
}

fn text(value: &Value) -> String {
    match value {
        Value::Null => "-".to_owned(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// One `key: value` line per field.
pub fn print_fields(object: &Value, keys: &[&str]) {
    let width = keys.iter().map(|key| key.len()).max().unwrap_or(0);
    for key in keys {
        println!("{key:<width$}  {}", text(&object[key]));
    }
}

/// A left-aligned table of the named fields.
pub fn print_table(rows: &[Value], keys: &[&str]) {
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|row| keys.iter().map(|key| text(&row[key])).collect())
        .collect();
    let widths: Vec<usize> = keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            cells
                .iter()
                .map(|row| row[index].len())
                .chain([key.len()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |row: Vec<&str>| {
        let parts: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        println!("{}", parts.join("  ").trim_end());
    };
    line(keys.to_vec());
    for row in &cells {
        line(row.iter().map(String::as_str).collect());
    }
}
