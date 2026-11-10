//! Every fetch and store is journalled on its machine and shared, sealed, with the cache's
//! members, who read the team's numbers back; the server holds only ciphertext.

use oxabl_cache_client::{
    Activity, ActivityKind, ActivitySummary, Cache, FetchOptions, FetchRequest, Invite, Mode, me,
    round_trip,
};
use oxabl_cache_e2e::workdir::TestDir;

use crate::support::fixture::{self, key};
use crate::support::world::World;

fn pulls(activities: &[Activity]) -> Vec<&Activity> {
    activities
        .iter()
        .filter(|a| a.kind == ActivityKind::Pull)
        .collect()
}

fn pushes(activities: &[Activity]) -> Vec<&Activity> {
    activities
        .iter()
        .filter(|a| a.kind == ActivityKind::Push)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pulls_and_pushes_are_journalled_locally_and_read_back_by_every_member() {
    let world = World::start().await;
    let ci_dirs = TestDir::new("ci").expect("dir");
    let ci_config = world.config(&world.ci, Mode::Ci, &ci_dirs.path().join("state"));
    let ci = Cache::open(ci_config.clone()).await.expect("open ci");
    let sets = fixture::sets(&ci_dirs.path().join("workspace"), 12, "activity");
    let uploader = ci.uploader().expect("uploader");
    for set in &sets {
        uploader.put(set.key, set.refs()).await;
    }
    let stored = uploader.finish().await;

    let dev_dirs = TestDir::new("dev").expect("dir");
    let dev_config = world.config(&world.dev, Mode::Developer, &dev_dirs.path().join("state"));
    let dev = Cache::open(dev_config.clone()).await.expect("open dev");
    let mut requests: Vec<FetchRequest> = sets
        .iter()
        .map(|s| FetchRequest {
            key: s.key,
            expect: None,
        })
        .collect();
    requests.extend((0..4).map(|n| FetchRequest {
        key: key(90_000 + n),
        expect: None,
    }));
    let mut fetch = dev.fetch(requests, FetchOptions::new(dev_dirs.path().join("stage")));
    while fetch.next().await.is_some() {}

    let mine = ci_config.local_activity();
    assert_eq!(mine.len(), 1, "{mine:?}");
    assert_eq!(mine[0].kind, ActivityKind::Push);
    assert_eq!(mine[0].mode, Mode::Ci);
    assert_eq!(
        (mine[0].requested, mine[0].hits, mine[0].failed),
        (12, 12, 0)
    );
    assert_eq!(mine[0].bytes, stored.bytes_uploaded);

    let theirs = dev_config.local_activity();
    assert_eq!(theirs.len(), 1, "{theirs:?}");
    let pull = theirs[0];
    assert_eq!(pull.kind, ActivityKind::Pull);
    assert_eq!(
        (pull.requested, pull.hits, pull.misses, pull.failed),
        (16, 12, 4, 0)
    );
    let installed: u64 = sets
        .iter()
        .flat_map(|s| &s.files)
        .map(|f| f.bytes.len() as u64)
        .sum();
    assert_eq!(pull.bytes, installed, "plaintext bytes installed");

    let racing_state = dev_dirs.path().join("racing-state");
    std::fs::create_dir_all(racing_state.join("journal")).expect("dir");
    for entry in std::fs::read_dir(dev_dirs.path().join("state/journal")).expect("journal") {
        let entry = entry.expect("entry");
        std::fs::copy(
            entry.path(),
            racing_state.join("journal").join(entry.file_name()),
        )
        .expect("copy");
    }
    ci.share_activity().await;
    dev.share_activity().await;
    dev.share_activity().await;
    let racing = Cache::open(world.config(&world.dev, Mode::Developer, &racing_state))
        .await
        .expect("a second process with the same unsent journal");
    racing.share_activity().await;

    let reader = world.machine(&world.reader, Mode::ReadOnly).await;
    let team = reader.cache.team_activity().await.expect("team");
    assert_eq!(pushes(&team).len(), 1, "{team:?}");
    assert_eq!(
        pulls(&team).len(),
        1,
        "shared twice, counted once: {team:?}"
    );
    let shared = pushes(&team)[0];
    assert_eq!(
        (shared.at, shared.requested, shared.hits, shared.bytes),
        (mine[0].at, mine[0].requested, mine[0].hits, mine[0].bytes)
    );
    assert_eq!(pulls(&team)[0].requested, 16);
    assert!(
        team.iter().all(|a| a.rtt_ms > 0),
        "each client measured its round trip: {team:?}"
    );

    let summary = ActivitySummary::of(&team);
    assert_eq!((summary.pulls, summary.pushes), (1, 1));
    assert_eq!(summary.hit_rate, Some(12.0 / 16.0));
    assert_eq!(summary.bytes_pulled, installed);
    assert_eq!(summary.bytes_pushed, stored.bytes_uploaded);
    assert!(summary.rtt_ms.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_member_removed_with_a_new_epoch_shares_nothing_the_new_epoch_reads() {
    let mut world = World::start().await;
    let dev_dirs = TestDir::new("dev").expect("dir");
    let dev_config = world.config(&world.dev, Mode::Developer, &dev_dirs.path().join("state"));
    let dev = Cache::open(dev_config).await.expect("open dev");
    let mut fetch = dev.fetch(
        vec![FetchRequest {
            key: key(1),
            expect: None,
        }],
        FetchOptions::new(dev_dirs.path().join("stage")),
    );
    while fetch.next().await.is_some() {}
    dev.share_activity().await;
    let before = world.machine(&world.reader, Mode::ReadOnly).await;
    assert_eq!(before.cache.team_activity().await.expect("team").len(), 1);

    world.remove_member("dev2").await;
    let after = world.machine(&world.reader, Mode::ReadOnly).await;
    assert!(
        after.cache.team_activity().await.expect("team").is_empty(),
        "reports sealed under the old epoch's key are not read in the new one"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sample_fetch_is_not_recorded_as_a_build() {
    let world = World::start().await;
    let ci = world.machine(&world.ci, Mode::Ci).await;
    let sets = fixture::sets(&ci.workspace(), 3, "sample");
    ci.store(&sets).await;
    let dirs = TestDir::new("verify").expect("dir");
    let config = world.config(&world.reader, Mode::ReadOnly, &dirs.path().join("state"));
    let reader = Cache::open(config.clone()).await.expect("open");
    let mut sample = reader
        .fetch_sample(3, FetchOptions::new(dirs.path().join("stage")))
        .await
        .expect("sample");
    while sample.next().await.is_some() {}
    assert!(config.local_activity().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn me_names_the_caches_of_the_credential_and_the_server_answers_a_round_trip() {
    let world = World::start().await;
    let dirs = TestDir::new("me").expect("dir");
    let config = world.config(&world.dev, Mode::Developer, &dirs.path().join("state"));
    let seen = me(&config).await.expect("me");
    assert_eq!(seen.tenant, world.tenant);
    assert!(!seen.tenant_admin);
    assert_eq!(seen.caches.len(), 1);
    assert_eq!(seen.caches[0].id, world.cache);
    assert_eq!(seen.caches[0].display_name, "codebase");
    assert_eq!(seen.caches[0].scopes, ["read", "write:personal"]);
    assert_eq!(seen.caches[0].roster_version, 1);
    assert!(
        round_trip(&config, 3)
            .await
            .expect("round trip")
            .as_millis()
            < 5_000
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_invite_carries_the_pins_and_opens_the_same_cache() {
    let world = World::start().await;
    let dirs = TestDir::new("invite").expect("dir");
    let pinned = world.config(&world.dev, Mode::Developer, &dirs.path().join("state"));
    let invite = Invite::new(
        &pinned,
        "Codebase",
        Some(("https://login.example.com/", "oxabl-desktop")),
    )
    .expect("invite");
    let text = invite.to_text();
    assert!(text.starts_with("oxc-invite:"));
    assert!(!text.contains(&world.dev.token) && !text.contains(&world.dev.identity));
    let read = Invite::parse(&format!("  {text}\n")).expect("parse");
    assert_eq!(read, invite);
    assert_eq!(read.name(), "Codebase");
    assert_eq!(read.cache(), world.cache);
    assert_eq!(
        read.sign_in(),
        Some(("https://login.example.com/", "oxabl-desktop"))
    );
    assert_eq!(
        read.cache_toml(),
        std::fs::read_to_string(world.repo().join(".oxabl/cache.toml")).expect("toml")
    );

    let config = read
        .config()
        .with_credentials(oxabl_cache_client::Credentials::Token(
            oxabl_cache_client::SecretString::from(world.dev.token.clone()),
        ))
        .with_identity(oxabl_cache_client::IdentitySource::Value(
            oxabl_cache_client::SecretString::from(world.dev.identity.clone()),
        ))
        .with_state_dir(dirs.path().join("state"));
    let cache = Cache::open(config).await.expect("open from the invite");
    assert_eq!(cache.mode(), Mode::Developer);

    let damaged: Vec<String> = (11..text.len())
        .step_by(7)
        .map(|at| {
            let mut bytes = text.clone().into_bytes();
            bytes[at] = if bytes[at] == b'A' { b'B' } else { b'A' };
            String::from_utf8(bytes).expect("utf8")
        })
        .collect();
    for bad in damaged {
        assert!(
            Invite::parse(&bad).is_err(),
            "a changed character is refused: {bad}"
        );
    }
    for bad in [
        text[..text.len() - 3].to_owned(),
        text.replace("oxc-invite:", "oxc-invitation:"),
        String::new(),
        "oxc-invite:".to_owned(),
        format!("{text}{}", "A".repeat(9_000)),
    ] {
        assert!(Invite::parse(&bad).is_err(), "{bad:.40}");
    }
    let http = Invite::new(
        &pinned,
        "Codebase",
        Some(("http://login.example.com/", "oxabl-desktop")),
    );
    assert!(http.is_err(), "the issuer must be https");
}

/// The JSON inside an invite, and an invite of any JSON with a valid checksum.
fn invite_json(text: &str) -> serde_json::Value {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(text.strip_prefix("oxc-invite:").expect("prefix"))
        .expect("base64");
    serde_json::from_slice(&bytes[..bytes.len() - 4]).expect("json")
}

fn invite_of(json: &serde_json::Value) -> String {
    use base64::Engine as _;
    let mut bytes = serde_json::to_vec(json).expect("json");
    let check = blake3::hash(&bytes);
    bytes.extend_from_slice(&check.as_bytes()[..4]);
    format!(
        "oxc-invite:{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_invite_without_a_sign_in_carries_only_the_pins() {
    let world = World::start().await;
    let dirs = TestDir::new("invite").expect("dir");
    let pinned = world.config(&world.dev, Mode::Developer, &dirs.path().join("state"));
    let invite = Invite::new(&pinned, "Codebase", None).expect("invite");
    let text = invite.to_text();
    let json = invite_json(&text);
    assert!(json.get("issuer").is_none() && json.get("client_id").is_none());
    let read = Invite::parse(&text).expect("parse");
    assert_eq!(read, invite);
    assert_eq!(read.sign_in(), None);
    assert_eq!(read.cache(), world.cache);

    let config = read
        .config()
        .with_credentials(oxabl_cache_client::Credentials::Token(
            oxabl_cache_client::SecretString::from(world.dev.token.clone()),
        ))
        .with_identity(oxabl_cache_client::IdentitySource::Value(
            oxabl_cache_client::SecretString::from(world.dev.identity.clone()),
        ))
        .with_state_dir(dirs.path().join("state"));
    Cache::open(config).await.expect("open from the invite");

    let signed = Invite::new(
        &pinned,
        "Codebase",
        Some(("https://login.example.com/", "oxabl-desktop")),
    )
    .expect("invite");
    let full = invite_json(&signed.to_text());
    for half in ["issuer", "client_id"] {
        let mut json = full.clone();
        json.as_object_mut().expect("object").remove(half);
        assert!(
            Invite::parse(&invite_of(&json)).is_err(),
            "an invite without its {half} is refused"
        );
    }
    assert_eq!(Invite::parse(&invite_of(&full)).expect("parse"), signed);
}
