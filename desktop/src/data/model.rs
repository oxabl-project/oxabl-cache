use oxabl_cache_client::{Activity, ActivityKind, ActivitySummary};
use serde::{Deserialize, Serialize};

/// Where a cache stands for this person on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Standing {
    /// Opened and verified: everything below is current.
    Member { role: String },
    /// Joined here, not yet in the roster: an owner still has to add this machine.
    AwaitingOwner,
    /// The organisation's sign-in has expired or never happened on this machine.
    SignedOut,
    /// The identity is locked: the keyring holds no passphrase for it.
    Locked,
    /// Visible to the signed-in person, not joined on this machine.
    Available,
    /// The last attempt failed for a reason a person can read.
    Trouble(String),
    /// Not asked yet in this run.
    Loading,
}

/// The numbers one side (the team, or this machine) shows for a cache.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Numbers {
    pub pulls: u32,
    pub pushes: u32,
    pub pull_ms: Option<u32>,
    pub push_ms: Option<u32>,
    pub hit_rate: Option<f64>,
    pub rtt_ms: Option<u32>,
    pub pull_bytes_per_second: Option<u64>,
}

impl Numbers {
    pub fn of(activities: &[Activity]) -> Numbers {
        let summary = ActivitySummary::of(activities);
        Numbers {
            pulls: summary.pulls,
            pushes: summary.pushes,
            pull_ms: summary.pull_ms,
            push_ms: summary.push_ms,
            hit_rate: summary.hit_rate,
            rtt_ms: summary.rtt_ms,
            pull_bytes_per_second: summary.pull_bytes_per_second,
        }
    }
}

/// The wall times of the most recent pulls, oldest first, for a small chart.
pub fn recent_pull_times(activities: &[Activity], count: usize) -> Vec<u32> {
    let mut pulls: Vec<&Activity> = activities
        .iter()
        .filter(|a| a.kind == ActivityKind::Pull && a.requested > 0)
        .collect();
    pulls.sort_by_key(|a| a.at);
    pulls
        .iter()
        .rev()
        .take(count)
        .rev()
        .map(|a| a.duration_ms)
        .collect()
}

/// One cache as the window shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheCard {
    /// `c_…`.
    pub id: String,
    /// `t_…`.
    pub tenant: String,
    pub name: String,
    /// The server's host, which stands for the organisation.
    pub host: String,
    pub standing: Standing,
    pub live_bytes: Option<u64>,
    pub hard_bytes: Option<u64>,
    /// The fastest of a few round trips just now.
    pub latency_ms: Option<u32>,
    pub team: Numbers,
    pub mine: Numbers,
    pub team_recent: Vec<u32>,
    pub mine_recent: Vec<u32>,
    /// Unix seconds of the last refresh that reached the server.
    pub refreshed_at: Option<u64>,
}

impl CacheCard {
    pub fn new(id: &str, tenant: &str, name: &str, host: &str, standing: Standing) -> CacheCard {
        CacheCard {
            id: id.to_owned(),
            tenant: tenant.to_owned(),
            name: name.to_owned(),
            host: host.to_owned(),
            standing,
            live_bytes: None,
            hard_bytes: None,
            latency_ms: None,
            team: Numbers::default(),
            mine: Numbers::default(),
            team_recent: Vec::new(),
            mine_recent: Vec::new(),
            refreshed_at: None,
        }
    }

    pub fn is_joined(&self) -> bool {
        !matches!(self.standing, Standing::Available)
    }
}
