//! Prometheus metrics: counters, gauges and histograms rendered in the text format.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Mutex;

const BUCKETS: [f64; 11] = [
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 10.0,
];

#[derive(Debug)]
enum Value {
    Counter(u64),
    Gauge(f64),
    Histogram {
        counts: [u64; BUCKETS.len()],
        sum: f64,
        total: u64,
    },
}

type Key = (&'static str, String);

/// A registry of labelled series. Label values come from fixed vocabularies, never
/// from ids, so the series count stays bounded.
#[derive(Debug, Default)]
pub struct Metrics {
    series: Mutex<BTreeMap<Key, Value>>,
}

fn key(name: &'static str, labels: &[(&str, &str)]) -> Key {
    let mut rendered = String::new();
    for (index, (label, value)) in labels.iter().enumerate() {
        if index > 0 {
            rendered.push(',');
        }
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        let _ = write!(rendered, "{label}=\"{escaped}\"");
    }
    (name, rendered)
}

impl Metrics {
    fn with(&self, key: Key, update: impl FnOnce(Option<&mut Value>) -> Option<Value>) {
        let mut series = self.series.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = update(series.get_mut(&key)) {
            series.insert(key, value);
        }
    }

    /// Adds `by` to a counter.
    pub fn inc(&self, name: &'static str, labels: &[(&str, &str)], by: u64) {
        self.with(key(name, labels), |found| match found {
            Some(Value::Counter(total)) => {
                *total += by;
                None
            }
            _ => Some(Value::Counter(by)),
        });
    }

    /// Sets a gauge.
    pub fn set(&self, name: &'static str, labels: &[(&str, &str)], value: f64) {
        self.with(key(name, labels), |found| match found {
            Some(Value::Gauge(current)) => {
                *current = value;
                None
            }
            _ => Some(Value::Gauge(value)),
        });
    }

    /// Records one observation, in seconds, in a histogram.
    pub fn observe(&self, name: &'static str, labels: &[(&str, &str)], seconds: f64) {
        self.with(key(name, labels), |found| {
            let add = |counts: &mut [u64; BUCKETS.len()], sum: &mut f64, total: &mut u64| {
                for (slot, bound) in counts.iter_mut().zip(BUCKETS) {
                    if seconds <= bound {
                        *slot += 1;
                    }
                }
                *sum += seconds;
                *total += 1;
            };
            match found {
                Some(Value::Histogram { counts, sum, total }) => {
                    add(counts, sum, total);
                    None
                }
                _ => {
                    let (mut counts, mut sum, mut total) = ([0; BUCKETS.len()], 0.0, 0);
                    add(&mut counts, &mut sum, &mut total);
                    Some(Value::Histogram { counts, sum, total })
                }
            }
        });
    }

    /// The text exposition of every series.
    pub fn render(&self) -> String {
        let series = self.series.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = String::new();
        let mut current = "";
        for ((name, labels), value) in series.iter() {
            if *name != current {
                let kind = match value {
                    Value::Counter(_) => "counter",
                    Value::Gauge(_) => "gauge",
                    Value::Histogram { .. } => "histogram",
                };
                let _ = writeln!(out, "# TYPE {name} {kind}");
                current = name;
            }
            let braces = |extra: &str| match (labels.is_empty(), extra.is_empty()) {
                (true, true) => String::new(),
                (true, false) => format!("{{{extra}}}"),
                (false, true) => format!("{{{labels}}}"),
                (false, false) => format!("{{{labels},{extra}}}"),
            };
            match value {
                Value::Counter(total) => {
                    let _ = writeln!(out, "{name}{} {total}", braces(""));
                }
                Value::Gauge(current) => {
                    let _ = writeln!(out, "{name}{} {current}", braces(""));
                }
                Value::Histogram { counts, sum, total } => {
                    for (count, bound) in counts.iter().zip(BUCKETS) {
                        let _ = writeln!(
                            out,
                            "{name}_bucket{} {count}",
                            braces(&format!("le=\"{bound}\""))
                        );
                    }
                    let _ = writeln!(out, "{name}_bucket{} {total}", braces("le=\"+Inf\""));
                    let _ = writeln!(out, "{name}_sum{} {sum}", braces(""));
                    let _ = writeln!(out, "{name}_count{} {total}", braces(""));
                }
            }
        }
        out
    }
}
