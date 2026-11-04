//! `login` and `logout`: the device grant against a fake identity provider, and a session that
//! the server accepts, stores with mode 0600 and revokes.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use oxabl_cache_e2e::idp::ca::TestCa;
use oxabl_cache_e2e::idp::device::Device;
use oxabl_cache_e2e::idp::keys::{Alg, SigningKey};
use oxabl_cache_e2e::idp::server::Idp;
use oxabl_cache_e2e::netns;
use oxabl_cache_e2e::workdir::TestDir;
use serde_json::{Value, json};

use crate::support::person::Person;
use crate::support::world::World;

const ADDRESS: &str = "10.99.0.10";
const PREFIX: &str = "/people";
const CLIENT: &str = "oxabl-cache-cli";

struct Login {
    world: World,
    idp: Idp,
    key: Arc<SigningKey>,
    ca_file: String,
    issuer: String,
    cache: String,
    owner: Person,
    _ca_dir: TestDir,
}

fn device(key: &Arc<SigningKey>, audience: Option<&str>) -> Device {
    Device {
        client_id: CLIENT.to_owned(),
        key: key.clone(),
        subject: "alice".to_owned(),
        audience: audience.map(str::to_owned),
        pending_polls: 1,
        slow_down: true,
        deny: false,
    }
}

impl Login {
    async fn start() -> Self {
        let ca = TestCa::new();
        let ca_dir = TestDir::new("ca").expect("ca directory");
        let ca_path = ca_dir.path().join("ca.pem");
        fs::write(&ca_path, ca.pem()).expect("ca file");
        let idp = Idp::start(
            format!("{ADDRESS}:443").parse().expect("address"),
            &ca,
            &[ADDRESS],
        )
        .await;
        let key = Arc::new(SigningKey::generate(Alg::Es256, "k1"));
        idp.publish(PREFIX, vec![key.clone()]);
        idp.device(PREFIX, device(&key, None));
        let world = World::start_with(&format!(
            "allowed_private_ranges = [\"{ADDRESS}/32\"]\nextra_ca_file = \"{}\"\n",
            ca_path.display()
        ))
        .await;
        let issuer = format!("https://{ADDRESS}{PREFIX}");

        let mut owner = Person::new("alice");
        owner.use_repo(world.repo.path());
        let made = owner
            .cli(&["identity", "new", "--file", &owner.identity_arg()])
            .ok()
            .json();
        let cache = world
            .create_cache("codebase", made["signing_key_hex"].as_str().expect("key"))
            .await;
        let registered = world
            .server
            .api(
                &world.tenant_admin,
                "POST",
                "/v1/tenant/oidc/issuers",
                Some(&json!({"issuer": issuer, "shape": "custom", "id_claims": ["sub"]})),
            )
            .await;
        assert_eq!(registered.status, 201, "{}", registered.body);
        let user = world
            .server
            .api(
                &world.tenant_admin,
                "POST",
                "/v1/tenant/users",
                Some(&json!({"issuer": issuer, "subject": "alice", "label": "alice"})),
            )
            .await;
        assert_eq!(user.status, 201, "{}", user.body);
        let granted = world
            .server
            .api(
                &world.tenant_admin,
                "PUT",
                "/v1/tenant/grants",
                Some(&json!({
                    "principal": user.json()["id"],
                    "cache": cache,
                    "scopes": ["read", "cache:admin"],
                })),
            )
            .await;
        assert_eq!(granted.status, 204, "{}", granted.body);
        let cache_toml = format!(
            "server = \"{}\"\ntenant = \"{}\"\ncache = \"{cache}\"\nowner_keys = [\"{}\"]\nmin_roster_version = 0\n",
            world.url(),
            world.tenant,
            made["signing_key"].as_str().expect("key"),
        );
        world.write_repo_file(&cache_toml);
        Self {
            world,
            idp,
            key,
            ca_file: ca_path.display().to_string(),
            issuer,
            cache,
            owner,
            _ca_dir: ca_dir,
        }
    }

    fn login(&self) -> crate::support::cli::Output {
        self.owner.cli(&[
            "login",
            "--issuer",
            &self.issuer,
            "--client-id",
            CLIENT,
            "--ca-file",
            &self.ca_file,
        ])
    }

    fn session_file(&self) -> std::path::PathBuf {
        self.owner.state.join("session").join(&self.world.tenant)
    }

    fn exchanges(&self) -> usize {
        self.world.server.log().matches("/v1/auth/oidc").count()
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_person_logs_in_with_a_device_code_and_the_session_works_until_logout_revokes_it() {
    if !netns::enter(&[ADDRESS]) {
        return;
    }
    let env = Login::start().await;
    let output = env.login().ok();
    assert!(
        output.stderr.contains("WDJB-MJHT")
            && output.stderr.contains(&format!("{}/activate", env.issuer)),
        "the person is told where to go and what to type:\n{}",
        output.stderr
    );
    let logged_in = output.json();
    assert_eq!(logged_in["logged_in"], true);
    assert_eq!(env.exchanges(), 1, "the ID token was exchanged once");

    let asked = env.idp.device_requests(PREFIX);
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].contains(&format!("audience=oxabl-cache%3A{}", env.world.tenant))
            && asked[0].contains("scope=openid")
            && asked[0].contains(&format!("client_id={CLIENT}")),
        "{}",
        asked[0]
    );

    let file = env.session_file();
    assert_eq!(
        fs::metadata(&file).expect("session").permissions().mode() & 0o777,
        0o600
    );
    let stored: Value =
        serde_json::from_str(&fs::read_to_string(&file).expect("session")).expect("JSON");
    let lifetime = stored["expires_at"]
        .as_u64()
        .expect("expiry")
        .saturating_sub(now());
    assert!(
        (43_000..=43_260).contains(&lifetime),
        "a human session lasts 12 hours: {lifetime}"
    );
    assert_eq!(logged_in["expires_at"], stored["expires_at"]);
    let token = stored["token"].as_str().expect("token").to_owned();
    assert!(
        !output.everything().contains(&token),
        "the session token is never printed"
    );

    let init = env
        .owner
        .cli(&[
            "init",
            "--server",
            &env.world.url(),
            "--tenant",
            &env.world.tenant,
            "--cache",
            &env.cache,
            "--label",
            "alice",
        ])
        .ok()
        .json();
    assert_eq!(
        init["roster_version"], 1,
        "the stored session carried cache:admin"
    );
    let status = env.owner.cli(&["status"]).ok().json();
    assert_eq!(status["principal"]["label"], "alice");

    let live = env
        .world
        .server
        .api(&token, "GET", &format!("/v1/caches/{}", env.cache), None)
        .await;
    assert_eq!(live.status, 200);
    let out = env.owner.cli(&["logout"]).ok().json();
    assert_eq!(
        (out["removed"].as_bool(), out["revoked"].as_bool()),
        (Some(true), Some(true))
    );
    assert!(!file.exists(), "the session file is deleted");
    let dead = env
        .world
        .server
        .api(&token, "GET", &format!("/v1/caches/{}", env.cache), None)
        .await;
    assert_eq!(dead.status, 401, "the server no longer accepts the session");
    env.owner.cli(&["status"]).exits(2);
    let again = env.owner.cli(&["logout"]).ok().json();
    assert_eq!(again["removed"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_login_whose_id_token_has_another_audience_sends_nothing_to_the_server() {
    if !netns::enter(&[ADDRESS]) {
        return;
    }
    let env = Login::start().await;
    env.idp
        .device(PREFIX, device(&env.key, Some("sts.amazonaws.com")));
    let before = env.exchanges();
    let refused = env.login().exits(1);
    assert!(refused.stderr.contains("audience"), "{}", refused.stderr);
    assert_eq!(
        env.exchanges(),
        before,
        "the ID token never left the process"
    );
    assert!(!env.session_file().exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_denied_login_stores_nothing_and_fails() {
    if !netns::enter(&[ADDRESS]) {
        return;
    }
    let env = Login::start().await;
    let mut denied = device(&env.key, None);
    denied.deny = true;
    env.idp.device(PREFIX, denied);
    let refused = env.login().exits(1);
    assert!(refused.stderr.contains("denied"), "{}", refused.stderr);
    assert!(!env.session_file().exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_login_for_a_person_the_tenant_does_not_know_is_refused() {
    if !netns::enter(&[ADDRESS]) {
        return;
    }
    let env = Login::start().await;
    let mut stranger = device(&env.key, None);
    stranger.subject = "mallory".to_owned();
    stranger.pending_polls = 0;
    stranger.slow_down = false;
    env.idp.device(PREFIX, stranger);
    let refused = env.login().exits(1);
    assert!(refused.stderr.contains("session"), "{}", refused.stderr);
    assert!(!env.session_file().exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_discovery_document_that_names_another_issuer_is_refused() {
    if !netns::enter(&[ADDRESS]) {
        return;
    }
    let env = Login::start().await;
    env.idp.claim_issuer(PREFIX, "https://evil.example/people");
    let refused = env.login().exits(1);
    assert!(
        refused.stderr.contains("another issuer"),
        "{}",
        refused.stderr
    );
    assert!(
        env.idp.device_requests(PREFIX).is_empty(),
        "no device code was requested"
    );
    assert!(!env.session_file().exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_issuer_that_is_not_https_is_refused_before_anything_is_sent() {
    let world = World::start().await;
    let mut person = Person::new("someone");
    let checkout = world.checkout(&format!(
        "server = \"{}\"\ntenant = \"{}\"\ncache = \"c_00000000000000000000000000\"\nowner_keys = [\"ed25519:{}\"]\nmin_roster_version = 0\n",
        world.url(),
        world.tenant,
        "A".repeat(43),
    ));
    person.use_repo(checkout.path());
    let refused = person
        .cli(&[
            "login",
            "--issuer",
            "http://idp.example/people",
            "--client-id",
            CLIENT,
        ])
        .exits(1);
    assert!(
        refused.stderr.contains("must be https"),
        "{}",
        refused.stderr
    );
}
