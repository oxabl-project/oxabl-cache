use super::{Activity, ActivityKind};

/// Averages over a set of activities.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ActivitySummary {
    pub pulls: u32,
    pub pushes: u32,
    /// Mean wall time of a pull.
    pub pull_ms: Option<u32>,
    /// Mean wall time of a push.
    pub push_ms: Option<u32>,
    /// Entries installed over keys asked for, across every pull, from 0 to 1.
    pub hit_rate: Option<f64>,
    /// Median round trip to the server, over activities that measured one.
    pub rtt_ms: Option<u32>,
    /// Plaintext bytes installed per second of pulling.
    pub pull_bytes_per_second: Option<u64>,
    pub bytes_pulled: u64,
    pub bytes_pushed: u64,
    /// Keys and sets that failed for any reason other than a miss.
    pub failed: u64,
}

fn mean(values: impl Iterator<Item = u64>) -> Option<u32> {
    let (sum, count) = values.fold((0u128, 0u128), |(s, c), v| (s + u128::from(v), c + 1));
    (count > 0).then(|| u32::try_from(sum / count).unwrap_or(u32::MAX))
}

impl ActivitySummary {
    pub fn of(activities: &[Activity]) -> Self {
        let pulls: Vec<&Activity> = activities
            .iter()
            .filter(|a| a.kind == ActivityKind::Pull && a.requested > 0)
            .collect();
        let pushes: Vec<&Activity> = activities
            .iter()
            .filter(|a| a.kind == ActivityKind::Push && a.requested > 0)
            .collect();
        let requested: u64 = pulls.iter().map(|a| u64::from(a.requested)).sum();
        let hits: u64 = pulls.iter().map(|a| u64::from(a.hits)).sum();
        let bytes_pulled: u64 = pulls.iter().map(|a| a.bytes).sum();
        let pull_millis: u64 = pulls.iter().map(|a| u64::from(a.duration_ms)).sum();
        let mut rtts: Vec<u32> = activities
            .iter()
            .map(|a| a.rtt_ms)
            .filter(|ms| *ms > 0)
            .collect();
        rtts.sort_unstable();
        Self {
            pulls: u32::try_from(pulls.len()).unwrap_or(u32::MAX),
            pushes: u32::try_from(pushes.len()).unwrap_or(u32::MAX),
            pull_ms: mean(pulls.iter().map(|a| u64::from(a.duration_ms))),
            push_ms: mean(pushes.iter().map(|a| u64::from(a.duration_ms))),
            hit_rate: (requested > 0).then(|| hits as f64 / requested as f64),
            rtt_ms: rtts.get(rtts.len() / 2).copied(),
            pull_bytes_per_second: (pull_millis > 0 && bytes_pulled > 0)
                .then(|| bytes_pulled.saturating_mul(1_000) / pull_millis),
            bytes_pulled,
            bytes_pushed: pushes.iter().map(|a| a.bytes).sum(),
            failed: activities.iter().map(|a| u64::from(a.failed)).sum(),
        }
    }
}
