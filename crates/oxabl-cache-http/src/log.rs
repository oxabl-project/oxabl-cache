//! JSON lines on stderr. Callers name every field they log; nothing is dumped
//! wholesale, so a token, a claim or a query string cannot slip into a line.

use serde_json::{Map, Value};
use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Severity of a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Verbose diagnostics.
    Debug = 0,
    /// Routine events.
    Info = 1,
    /// Something unexpected that the service handled.
    Warn = 2,
    /// A failure.
    Error = 3,
}

impl Level {
    /// Parses `debug`, `info`, `warn` or `error`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

static THRESHOLD: AtomicU8 = AtomicU8::new(Level::Info as u8);

/// Sets the lowest level that is written.
pub fn set_level(level: Level) {
    THRESHOLD.store(level as u8, Ordering::Relaxed);
}

/// Writes one line `{"ts":…,"level":…,"event":…,…fields}` if `level` passes the threshold.
pub fn emit(level: Level, event: &str, fields: Vec<(&str, Value)>) {
    if (level as u8) < THRESHOLD.load(Ordering::Relaxed) {
        return;
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let mut line = Map::new();
    line.insert("ts".into(), Value::from(u64::try_from(millis).unwrap_or(0)));
    line.insert("level".into(), Value::from(level.name()));
    line.insert("event".into(), Value::from(event));
    for (name, value) in fields {
        line.insert(name.into(), value);
    }
    let mut text = Value::Object(line).to_string();
    text.push('\n');
    let _ = std::io::stderr().lock().write_all(text.as_bytes());
}
