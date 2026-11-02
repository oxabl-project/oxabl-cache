//! Durations (`200ms`, `10m`, `24h`, `7d`) and byte sizes (`20GiB`) as written in the config.

use std::time::Duration;

fn split(text: &str) -> Option<(u64, &str)> {
    let end = text.find(|c: char| !c.is_ascii_digit())?;
    let (number, unit) = text.split_at(end);
    Some((number.parse().ok()?, unit.trim()))
}

/// Parses a duration with a unit of `ms`, `s`, `m`, `h` or `d`.
pub(crate) fn duration(text: &str) -> Result<Duration, String> {
    let bad = || format!("`{text}` is not a duration such as 200ms, 10m, 24h or 7d");
    let (number, unit) = split(text.trim()).ok_or_else(bad)?;
    let factor = match unit {
        "ms" => return Ok(Duration::from_millis(number)),
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => return Err(bad()),
    };
    Ok(Duration::from_secs(number.saturating_mul(factor)))
}

/// Parses a byte size: plain bytes, or a number with a binary (`GiB`) or decimal (`GB`) unit.
pub(crate) fn size(text: &str) -> Result<u64, String> {
    let bad = || format!("`{text}` is not a size such as 512MiB or 20GiB");
    let trimmed = text.trim();
    if let Ok(bytes) = trimmed.parse::<u64>() {
        return Ok(bytes);
    }
    let (number, unit) = split(trimmed).ok_or_else(bad)?;
    let factor: u64 = match unit {
        "B" => 1,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        "TiB" => 1 << 40,
        "KB" => 1_000,
        "MB" => 1_000_000,
        "GB" => 1_000_000_000,
        "TB" => 1_000_000_000_000,
        _ => return Err(bad()),
    };
    number.checked_mul(factor).ok_or_else(bad)
}
