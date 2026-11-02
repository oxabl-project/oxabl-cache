//! `OXABL_CACHED_<SECTION>_<KEY>` overrides, applied to the parsed file before validation.

use toml::{Table, Value};

const PREFIX: &str = "OXABL_CACHED_";

fn scalar(text: &str) -> Value {
    toml::from_str::<Table>(&format!("v = {text}"))
        .ok()
        .and_then(|mut table| table.remove("v"))
        .unwrap_or_else(|| Value::String(text.to_owned()))
}

/// Sets `[section] key = value` for every override in `vars`. A variable naming no
/// section and key (`OXABL_CACHED_X`) is an error, so a typo is never silent.
pub(crate) fn overlay(
    table: &mut Table,
    vars: impl Iterator<Item = (String, String)>,
) -> Result<(), String> {
    for (name, value) in vars {
        let Some(rest) = name.strip_prefix(PREFIX) else {
            continue;
        };
        let rest = rest.to_ascii_lowercase();
        let (section, key) = rest
            .split_once('_')
            .ok_or_else(|| format!("{name} is not OXABL_CACHED_<SECTION>_<KEY>"))?;
        let section = table
            .entry(section.to_owned())
            .or_insert_with(|| Value::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| format!("{name}: [{section}] is not a table"))?;
        section.insert(key.to_owned(), scalar(&value));
    }
    Ok(())
}
