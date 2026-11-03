//! Sizes and durations as operators write them.

use crate::error::Failure;

/// Parses `20GiB`, `512MiB`, `4096` (bytes); binary units only. `none` is `None`.
pub fn parse_size(text: &str) -> Result<Option<u64>, Failure> {
    if text == "none" {
        return Ok(None);
    }
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, unit) = text.split_at(split);
    let factor: u64 = match unit {
        "" | "B" => 1,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        "TiB" => 1 << 40,
        _ => return Err(bad_size(text)),
    };
    let value: u64 = digits.parse().map_err(|_| bad_size(text))?;
    value
        .checked_mul(factor)
        .map(Some)
        .ok_or_else(|| bad_size(text))
}

fn bad_size(text: &str) -> Failure {
    Failure::usage(format!(
        "invalid size '{text}': use an integer with B, KiB, MiB, GiB or TiB, or 'none'"
    ))
}

/// Parses `30d` into whole days.
pub fn parse_days(text: &str) -> Result<i64, Failure> {
    text.strip_suffix('d')
        .and_then(|days| days.parse::<i64>().ok())
        .filter(|days| *days > 0)
        .ok_or_else(|| {
            Failure::usage(format!(
                "invalid duration '{text}': use whole days, such as 30d"
            ))
        })
}

/// Checks `YYYY-MM`.
pub fn check_month(text: &str) -> Result<(), Failure> {
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 7
        && bytes[4] == b'-'
        && bytes[..4].iter().chain(&bytes[5..]).all(u8::is_ascii_digit);
    let month: u32 = text.get(5..).and_then(|m| m.parse().ok()).unwrap_or(0);
    if shaped && (1..=12).contains(&month) {
        Ok(())
    } else {
        Err(Failure::usage(format!(
            "invalid month '{text}': use YYYY-MM"
        )))
    }
}
