//! Joining a cache with nothing but the window: paste the invite, approve the sign-in, send the
//! request to an owner.

use std::fs;
use std::sync::Arc;

use oxabl_cache_client::{Role, SecretString, open_identity_file, stored_passphrase};
use oxabl_cache_e2e::idp::ca::TestCa;
use oxabl_cache_e2e::idp::device::Device;
use oxabl_cache_e2e::idp::keys::{Alg, SigningKey};
use oxabl_cache_e2e::idp::server::Idp;
use oxabl_cache_e2e::netns;
use oxabl_cache_e2e::secret_service::SecretService;

use crate::support::window::Window;
use crate::support::world::{CLIENT_ID, ISSUER_PREFIX, Org, Person};

const ADDRESS: &str = "10.99.0.10";

#[test]
fn a_person_joins_with_an_invite_a_sign_in_and_an_owner_adding_them() {
    let service = (!netns::inside()).then(|| SecretService::start().expect("a secret service"));
    let environment = service
        .as_ref()
        .map(SecretService::environment)
        .unwrap_or_default();
    if !netns::enter_with(&[ADDRESS], &environment) {
        return;
    }
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let ca = TestCa::new();
    let person = Person::new("person");
    let ca_path = person.dir.path().join("ca.pem");
    fs::write(&ca_path, ca.pem()).expect("ca");
    let issuer = format!("https://{ADDRESS}{ISSUER_PREFIX}");
    let (org, _idp) = runtime.block_on(async {
        let idp = Idp::start(
            format!("{ADDRESS}:443").parse().expect("address"),
            &ca,
            &[ADDRESS],
        )
        .await;
        let key = Arc::new(SigningKey::generate(Alg::Es256, "k1"));
        idp.publish(ISSUER_PREFIX, vec![key.clone()]);
        idp.device(
            ISSUER_PREFIX,
            Device {
                client_id: CLIENT_ID.to_owned(),
                key,
                subject: "alice".to_owned(),
                audience: None,
                pending_polls: 2,
                slow_down: false,
                deny: false,
            },
        );
        let org = Org::start(
            &format!(
                "allowed_private_ranges = [\"{ADDRESS}/32\"]\nextra_ca_file = \"{}\"\n",
                ca_path.display()
            ),
            &issuer,
        )
        .await;
        (org, idp)
    });
    let cache = runtime.block_on(org.cache("ERP codebase"));
    runtime.block_on(org.person_signs_in_with("alice", &cache.id));
    let mut paths = person.paths.clone();
    paths.ca_file = Some(ca_path);

    let mut window = Window::open(&paths);
    window.wait_for("Your team's build cache");
    window.press("join");
    assert!(window.shows("Join a cache"));

    window
        .clipboard
        .set_text(&format!("  {}\n", cache.invite.to_text()));
    window.press("paste-invite");
    window.wait_until("the invite's preview", |w| {
        w.text_of("invite-name").as_deref() == Some("ERP codebase")
    });
    window.press("continue");

    window.wait_until("a sign-in code", |w| w.has("sign-in-code-text"));
    let code = window.text_of("sign-in-code-text").expect("code");
    window.press("copy-code");
    assert_eq!(window.copied().as_deref(), Some(code.as_str()));

    window.wait_for("Ask to be added");
    window.wait_until("the request to send", |w| w.has("request-text"));
    window.press("copy-request");
    let request = window.copied().expect("the request is on the clipboard");
    assert!(
        request.starts_with("oxabl-cache member add oxabl-cache-member-1"),
        "{request}"
    );
    assert!(request.ends_with("--role developer"), "{request}");

    window.press("copy-passphrase");
    let passphrase = window.copied().expect("the passphrase is on the clipboard");
    assert!(passphrase.len() >= 24, "a long random passphrase");
    assert!(
        open_identity_file(
            &paths.identity_file,
            &SecretString::from(passphrase.clone())
        )
        .is_ok(),
        "the copied passphrase unlocks the identity file"
    );
    assert_eq!(
        stored_passphrase(&paths.identity_file)
            .map(|p| oxabl_cache_client::ExposeSecret::expose_secret(&p).to_owned()),
        Some(passphrase),
        "and it is in the keyring"
    );
    assert!(window.shows("Waiting for an owner"));

    let words: Vec<&str> = request.split_whitespace().collect();
    runtime.block_on(org.add_member(&cache, words[3], words[5], Role::Developer));

    window.wait_for("You're in");
    window.press("done");
    window.wait_until("the joined cache with its numbers", |w| {
        w.shows("ERP codebase") && w.texts().iter().any(|t| t.ends_with(" ms"))
    });
    assert!(window.shows("1 cache"), "{:?}", window.texts());
    let settings = fs::read_to_string(&paths.config_file).expect("settings");
    assert!(
        settings.contains(&cache.invite.to_text()),
        "the join is remembered"
    );
}

#[test]
fn a_damaged_invite_is_refused_with_a_reason_and_nothing_is_remembered() {
    let person = Person::new("person");
    let mut window = Window::open(&person.paths);
    window.wait_for("Your team's build cache");
    window.press("join");
    window.paste_into("invite-field", "oxc-invite:eyJ2IjoxfQ");
    window.press("continue");
    assert!(
        window.shows("isn't a complete invite"),
        "{:?}",
        window.texts()
    );
    window.clipboard.set_text("hello");
    window.press("paste-invite");
    assert!(
        window.shows("doesn't hold an invite"),
        "{:?}",
        window.texts()
    );
    window.key(rune::Key::Escape);
    window.wait_for("Your team's build cache");
    assert!(!person.paths.config_file.exists());
}
