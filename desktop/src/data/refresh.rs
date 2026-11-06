use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Instant;

use oxabl_cache_client::{
    Cache, Config, FetchOptions, FetchOutcome, IdentitySource, Invite, OpenError, me, role_name,
    round_trip,
};

use super::model::{CacheCard, Numbers, Standing, recent_pull_times};
use super::worker::{Context, Event};

const RECENT: usize = 30;
const SPEED_SAMPLE: usize = 8;

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The invite's cache as this machine opens it: its state directory, its CA bundle and, once
/// unlocked, its identity.
pub(crate) fn config_of(context: &Context, invite: &Invite) -> Config {
    let mut config = invite
        .config()
        .with_state_dir(context.paths.state_dir.clone());
    if let Some(ca) = &context.paths.ca_file {
        config = config.with_ca_file(ca.clone());
    }
    if let Some(identity) = context.identity() {
        config = config.with_identity(IdentitySource::Value(identity));
    }
    config
}

fn host_of(invite: &Invite) -> String {
    invite.server().host_str().unwrap_or("cache").to_owned()
}

/// What a failed open means for the person.
pub(crate) fn standing_of(error: &OpenError, host: &str) -> Standing {
    match error {
        OpenError::NoCredentials | OpenError::Authentication => Standing::SignedOut,
        OpenError::IdentityNotInRoster | OpenError::NoRoster => Standing::AwaitingOwner,
        OpenError::Unreachable => Standing::Trouble(format!("Can't reach {host}")),
        OpenError::CacheNotFound => {
            Standing::Trouble("This cache is gone, or you no longer have access".into())
        }
        OpenError::Roster(_) => {
            Standing::Trouble("The cache's member list failed verification".into())
        }
        other => Standing::Trouble(other.to_string()),
    }
}

async fn refresh_one(context: &Context, invite: &Invite) -> CacheCard {
    let host = host_of(invite);
    let mut card = CacheCard::new(
        &invite.cache(),
        &invite.tenant(),
        invite.name(),
        &host,
        Standing::Loading,
    );
    let config = config_of(context, invite);
    let local = config.local_activity();
    card.mine = Numbers::of(&local);
    card.mine_recent = recent_pull_times(&local, RECENT);
    card.latency_ms = round_trip(&config, 3)
        .await
        .ok()
        .map(|d| u32::try_from(d.as_millis()).unwrap_or(u32::MAX).max(1));
    if context.identity().is_none() {
        card.standing = if context.paths.identity_file.exists() {
            Standing::Locked
        } else {
            Standing::AwaitingOwner
        };
        return card;
    }
    match Cache::open(config).await {
        Ok(cache) => {
            if let Ok(status) = cache.status().await {
                card.live_bytes = Some(status.server.quota.live_bytes);
                card.hard_bytes = Some(status.server.quota.hard_bytes);
                card.standing = Standing::Member {
                    role: role_name(status.principal.role).to_owned(),
                };
            } else {
                card.standing = Standing::Trouble(format!("{host} didn't answer"));
            }
            if let Ok(team) = cache.team_activity().await {
                card.team = Numbers::of(&team);
                card.team_recent = recent_pull_times(&team, RECENT);
            }
            card.refreshed_at = Some(now());
        }
        Err(error) => card.standing = standing_of(&error, &host),
    }
    card
}

/// Refreshes every joined cache in parallel, sending each card as soon as it is ready, then
/// lists the caches the signed-in person can see in each organisation and has not joined here.
pub(crate) async fn refresh_all(context: &Arc<Context>, invites: &[String]) {
    let parsed: Vec<Invite> = invites
        .iter()
        .filter_map(|t| Invite::parse(t).ok())
        .collect();
    let mut running = tokio::task::JoinSet::new();
    for invite in parsed.clone() {
        let context = context.clone();
        running.spawn(async move {
            let card = refresh_one(&context, &invite).await;
            context.send(Event::Card(Box::new(card.clone())));
            card
        });
    }
    let mut joined_cards = BTreeMap::new();
    while let Some(done) = running.join_next().await {
        if let Ok(card) = done {
            joined_cards.insert(card.id.clone(), card);
        }
    }
    let mut ids: Vec<String> = joined_cards.keys().cloned().collect();
    let joined: BTreeSet<String> = ids.iter().cloned().collect();
    let mut by_tenant: BTreeMap<String, &Invite> = BTreeMap::new();
    for invite in &parsed {
        by_tenant.entry(invite.tenant()).or_insert(invite);
    }
    for invite in by_tenant.values() {
        let Ok(seen) = me(&config_of(context, invite)).await else {
            continue;
        };
        for visible in seen.caches {
            if let Some(card) = joined_cards.get(&visible.id) {
                if card.live_bytes.is_none() {
                    let mut sized = card.clone();
                    sized.live_bytes = Some(visible.quota.live_bytes);
                    sized.hard_bytes = Some(visible.quota.hard_bytes);
                    context.send(Event::Card(Box::new(sized)));
                }
                continue;
            }
            if joined.contains(&visible.id) || visible.scopes.is_empty() {
                continue;
            }
            let mut card = CacheCard::new(
                &visible.id,
                &seen.tenant,
                &visible.display_name,
                &host_of(invite),
                Standing::Available,
            );
            card.live_bytes = Some(visible.quota.live_bytes);
            card.hard_bytes = Some(visible.quota.hard_bytes);
            ids.push(card.id.clone());
            context.send(Event::Card(Box::new(card)));
        }
    }
    context.send(Event::Refreshed { at: now(), ids });
}

/// Fetches a few entries the server picks, with every check a build makes, into a scratch
/// directory that is removed afterwards, and reports the plaintext bytes per second.
pub(crate) async fn speed_test(context: &Context, invite: &Invite) {
    let cache_id = invite.cache();
    let measured = async {
        let cache = Cache::open(config_of(context, invite)).await.ok()?;
        let stage = context
            .paths
            .state_dir
            .join(format!("desktop-speed-{}", std::process::id()));
        let started = Instant::now();
        let mut fetch = cache
            .fetch_sample(SPEED_SAMPLE, FetchOptions::new(stage.clone()))
            .await
            .ok()?;
        let mut bytes = 0u64;
        while let Some(outcome) = fetch.next().await {
            if let FetchOutcome::Installed { files, .. } = outcome {
                bytes += files.iter().map(|f| f.size).sum::<u64>();
            }
        }
        let elapsed = started.elapsed().as_secs_f64();
        let _ = std::fs::remove_dir_all(&stage);
        (bytes > 0 && elapsed > 0.0).then(|| (bytes as f64 / elapsed) as u64)
    }
    .await;
    context.send(Event::Speed {
        cache: cache_id,
        bytes_per_second: measured,
    });
}
