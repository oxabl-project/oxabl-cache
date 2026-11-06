//! Example caches for the named scenes the headless renders show; never shown to a person.

use crate::data::model::{CacheCard, Numbers, Standing};
use crate::data::settings::Appearance;

use super::state::{JoinStep, Screen, Side, Speed, Ui};

fn card(
    id: &str,
    name: &str,
    live: u64,
    hard: u64,
    latency: u32,
    hits: f64,
    pull: u32,
) -> CacheCard {
    let mut card = CacheCard::new(
        id,
        "t_demo",
        name,
        "cache.oxabl.org",
        Standing::Member {
            role: "developer".into(),
        },
    );
    card.live_bytes = Some(live);
    card.hard_bytes = Some(hard);
    card.latency_ms = Some(latency);
    card.team = Numbers {
        pulls: 214,
        pushes: 37,
        pull_ms: Some(pull),
        push_ms: Some(pull * 3),
        hit_rate: Some(hits),
        rtt_ms: Some(latency),
        pull_bytes_per_second: Some(48_000_000),
    };
    card.mine = Numbers {
        pulls: 19,
        pushes: 6,
        pull_ms: Some(pull + 600),
        push_ms: Some(pull * 2),
        hit_rate: Some(hits - 0.08),
        rtt_ms: Some(latency + 4),
        pull_bytes_per_second: Some(31_000_000),
    };
    card.team_recent = vec![
        2100, 1800, 2400, 900, 1300, 3100, 2000, 1700, 1500, 1200, 2600, 1900, 1100, 1400, 900,
        1600, 2200, 1000, 800, 1300,
    ];
    card.mine_recent = vec![3100, 2600, 2900, 1800, 2200, 2500, 1900, 2100];
    card.refreshed_at = Some(now() - 50);
    card
}

fn cards() -> Vec<CacheCard> {
    let mut available = CacheCard::new(
        "c_demo_wms",
        "t_demo",
        "Warehouse (WMS)",
        "cache.oxabl.org",
        Standing::Available,
    );
    available.live_bytes = Some(2_300_000_000);
    available.hard_bytes = Some(10_000_000_000);
    let mut waiting = CacheCard::new(
        "c_demo_tools",
        "t_demo",
        "Build tools",
        "cache.oxabl.org",
        Standing::AwaitingOwner,
    );
    waiting.latency_ms = Some(21);
    vec![
        card(
            "c_demo_erp",
            "ERP codebase",
            7_400_000_000,
            20_000_000_000,
            18,
            0.87,
            2400,
        ),
        card(
            "c_demo_portal",
            "Customer portal",
            1_100_000_000,
            5_000_000_000,
            22,
            0.93,
            900,
        ),
        waiting,
        available,
    ]
}

fn base(ui: &mut Ui) {
    ui.cards = cards();
    ui.refreshing = false;
    ui.refreshed_at = Some(now() - 50);
    ui.join.invite = demo_invite();
    ui.settings.effects = true;
    ui.settings.appearance = Appearance::Dark;
}

pub fn home(ui: &mut Ui) {
    base(ui);
}

pub fn home_light(ui: &mut Ui) {
    base(ui);
    ui.settings.appearance = Appearance::Light;
}

pub fn detail(ui: &mut Ui) {
    base(ui);
    ui.screen = Screen::Detail("c_demo_erp".into());
    ui.speed
        .insert("c_demo_erp".into(), Speed::Measured(Some(52_400_000)));
}

pub fn detail_light(ui: &mut Ui) {
    detail(ui);
    ui.settings.appearance = Appearance::Light;
    ui.side = Side::Mine;
}

pub fn welcome(ui: &mut Ui) {
    base(ui);
    ui.cards.clear();
}

/// A well-formed invite to a cache that does not exist.
fn invite_text() -> String {
    use base64::Engine as _;
    let key = base64::engine::general_purpose::STANDARD_NO_PAD.encode([7u8; 32]);
    let json = format!(
        r#"{{"v":1,"name":"ERP codebase","server":"https://cache.oxabl.org/","tenant":"{}","cache":"{}","owner_keys":["ed25519:{key}"],"min_roster_version":0,"issuer":"https://login.example.com/","client_id":"oxabl-desktop"}}"#,
        oxabl_cache_format::TenantId([3; 16]).to_text(),
        oxabl_cache_format::CacheId([5; 16]).to_text(),
    );
    let mut bytes = json.into_bytes();
    let check = blake3::hash(&bytes);
    bytes.extend_from_slice(&check.as_bytes()[..4]);
    format!(
        "oxc-invite:{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

fn demo_invite() -> Option<oxabl_cache_client::Invite> {
    oxabl_cache_client::Invite::parse(&invite_text()).ok()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn join_invite(ui: &mut Ui) {
    base(ui);
    ui.screen = Screen::Join;
    ui.join.edit = rune::TextEdit::new(invite_text());
}

pub fn join_sign_in(ui: &mut Ui) {
    base(ui);
    ui.screen = Screen::Join;
    ui.join.step = JoinStep::SignIn {
        url: Some("https://login.example.com/device?code=WDJB-MJHT".into()),
        code: Some("WDJB-MJHT".into()),
        failed: None,
    };
}

pub fn join_request(ui: &mut Ui) {
    base(ui);
    ui.screen = Screen::Join;
    ui.join.step = JoinStep::Request {
        command: Some("oxabl-cache member add oxabl-cache-member-1qyvx8v5z0mq3h4n7u2w9x6k3p8r5t0y4e7c2a9s6d3f0g5h8j2k7l4 --label evan@fedora --role developer".into()),
        passphrase: Some(oxabl_cache_client::SecretString::from("demo".to_owned())),
        failed: None,
    };
}

pub fn joined(ui: &mut Ui) {
    base(ui);
    ui.screen = Screen::Join;
    ui.join.step = JoinStep::Done;
}

pub fn settings(ui: &mut Ui) {
    base(ui);
    ui.screen = Screen::Settings;
}

pub fn unlock(ui: &mut Ui) {
    base(ui);
    ui.settings.appearance = Appearance::Light;
    ui.screen = Screen::Unlock;
}

/// A named state the headless renders show.
pub type Scene = (&'static str, fn(&mut Ui));

pub const SCENES: &[Scene] = &[
    ("home", home),
    ("home-light", home_light),
    ("detail", detail),
    ("detail-light", detail_light),
    ("welcome", welcome),
    ("join-invite", join_invite),
    ("join-sign-in", join_sign_in),
    ("join-request", join_request),
    ("joined", joined),
    ("settings", settings),
    ("unlock", unlock),
];
