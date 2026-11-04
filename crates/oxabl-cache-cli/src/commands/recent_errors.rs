//! The last logged failures of this cache, as `status` shows them.

use oxabl_cache_client::LoggedError;
use serde_json::{Value, json};

/// How many failures `status` shows.
pub const SHOWN: usize = 5;

pub fn json(errors: &[LoggedError]) -> Value {
    Value::Array(
        errors
            .iter()
            .map(|e| {
                json!({
                    "at": e.at,
                    "operation": e.operation,
                    "kind": e.kind,
                    "message": e.message,
                })
            })
            .collect(),
    )
}

/// `2026-11-04T13:58:20Z` for unix seconds.
fn utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// The `last errors` section, nothing when there are none.
pub fn print(errors: &[LoggedError]) {
    if errors.is_empty() {
        return;
    }
    println!("last errors");
    for e in errors {
        println!("  {}  {} {}: {}", utc(e.at), e.operation, e.kind, e.message);
    }
}
