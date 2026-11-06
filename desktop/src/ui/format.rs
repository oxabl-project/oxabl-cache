//! How numbers read: short, rounded, with units a person expects.

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

pub fn rate(bytes_per_second: u64) -> String {
    format!("{}/s", bytes(bytes_per_second))
}

pub fn millis(ms: u32) -> String {
    if ms < 1_000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", f64::from(ms) / 1_000.0)
    } else {
        format!("{}m {:02}s", ms / 60_000, (ms % 60_000) / 1_000)
    }
}

pub fn percent(rate: f64) -> String {
    format!("{:.0}%", (rate * 100.0).clamp(0.0, 100.0))
}

pub fn ago(then: u64, now: u64) -> String {
    let seconds = now.saturating_sub(then);
    match seconds {
        0..=44 => "just now".into(),
        45..=89 => "a minute ago".into(),
        90..=3_599 => format!("{} min ago", seconds / 60),
        3_600..=86_399 => format!("{} h ago", seconds / 3_600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

/// How a round trip feels: a word and how good it is, 0 (poor) to 2 (good).
pub fn connection(ms: u32) -> (&'static str, u8) {
    match ms {
        0..=60 => ("Excellent", 2),
        61..=150 => ("Good", 2),
        151..=350 => ("Fair", 1),
        _ => ("Slow", 0),
    }
}
