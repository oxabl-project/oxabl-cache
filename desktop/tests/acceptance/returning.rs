//! Coming back: a locked identity, the last numbers before any network, a server that is gone,
//! caches not joined here, and settings that stay.

use oxabl_cache_client::Role;
use oxabl_cache_e2e::reexec;
use oxabl_cache_e2e::secret_service::SecretService;

use crate::support::window::Window;
use crate::support::world::{Org, Person};

#[test]
fn a_locked_identity_is_unlocked_once_and_remembered() {
    let service = (!reexec::inside()).then(|| SecretService::start().expect("a secret service"));
    let environment = service
        .as_ref()
        .map(SecretService::environment)
        .unwrap_or_default();
    if !reexec::with_environment(&environment) {
        return;
    }
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let org = runtime.block_on(Org::start("", "https://login.example.com"));
    let cache = runtime.block_on(org.cache("ERP codebase"));
    let other = runtime.block_on(org.cache("Warehouse"));
    let person = Person::new("person");
    let identity = person.make_identity();
    let token = runtime.block_on(org.token(
        "alice",
        &["read", "write:personal"],
        &[&cache.id, &other.id],
    ));
    person.sign_in(&org.tenant, &token);
    runtime.block_on(org.add_member(
        &cache,
        &identity.descriptor_string().expect("descriptor"),
        "alice",
        Role::Developer,
    ));
    person.joined(&[&cache.invite]);

    let mut window = Window::open(&person.paths);
    window.wait_for("Unlock this computer");
    window.paste_into("passphrase-field", "not it");
    window.press("unlock-button");
    window.wait_for("doesn't unlock");
    window.paste_into("passphrase-field", &person.passphrase);
    window.press("unlock-button");
    window.wait_until("the cache's round trip", |w| {
        w.texts().iter().any(|t| t.ends_with(" ms"))
    });
    assert!(
        window.shows("ALSO IN YOUR ORGANISATION"),
        "{:?}",
        window.texts()
    );
    assert!(
        window.shows("Warehouse") && window.shows("Available"),
        "{:?}",
        window.texts()
    );

    window.press(&format!("card-{}", other.id));
    assert!(
        window.shows("Paste the invite for Warehouse"),
        "{:?}",
        window.texts()
    );
    drop(window);

    let mut again = Window::open(&person.paths);
    again.wait_until("the cache, without asking", |w| {
        w.texts().iter().any(|t| t.ends_with(" ms"))
    });
    assert!(!again.shows("Unlock this computer"));
}

#[test]
fn the_last_numbers_show_at_once_and_a_lost_server_is_said_plainly() {
    let service = (!reexec::inside()).then(|| SecretService::start().expect("a secret service"));
    let environment = service
        .as_ref()
        .map(SecretService::environment)
        .unwrap_or_default();
    if !reexec::with_environment(&environment) {
        return;
    }
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let org = runtime.block_on(Org::start("", "https://login.example.com"));
    let cache = runtime.block_on(org.cache("ERP codebase"));
    let person = Person::new("person");
    let identity = person.make_identity();
    oxabl_cache_client::save_passphrase(
        &person.paths.identity_file,
        &oxabl_cache_client::SecretString::from(person.passphrase.clone()),
    )
    .expect("keyring");
    let token = runtime.block_on(org.token("alice", &["read", "write:personal"], &[&cache.id]));
    person.sign_in(&org.tenant, &token);
    runtime.block_on(org.add_member(
        &cache,
        &identity.descriptor_string().expect("descriptor"),
        "alice",
        Role::Developer,
    ));
    person.joined(&[&cache.invite]);

    let mut first = Window::open(&person.paths);
    first.wait_until("the size of the cache", |w| {
        w.has(&format!("{}-size", cache.id))
    });
    let size = first.text_of(&format!("{}-size", cache.id)).expect("size");
    drop(first);
    drop(org);

    let cold = Window::open_without_network(&person.paths);
    assert!(
        cold.shows("ERP codebase"),
        "the first frame has the cache: {:?}",
        cold.texts()
    );
    assert_eq!(
        cold.text_of(&format!("{}-size", cache.id)),
        Some(size),
        "and its last size"
    );

    let mut offline = Window::open(&person.paths);
    offline.wait_for("Can't reach");
}

#[test]
fn appearance_and_effects_are_saved_and_come_back() {
    let person = Person::new("person");
    let mut window = Window::open(&person.paths);
    window.wait_for("Your team's build cache");
    let dark = window.backdrop_luminance();
    window.press("settings");
    window.press("appearance-light");
    window.press("effects");
    window.wait_until("a light window", |w| w.backdrop_luminance() > 0.5);
    assert!(dark < 0.1, "it opened dark, as the desktop asks");
    let saved = std::fs::read_to_string(&person.paths.config_file).expect("settings");
    assert!(saved.contains("appearance = \"light\""), "{saved}");
    assert!(saved.contains("effects = false"), "{saved}");
    drop(window);

    let again = Window::open_without_network(&person.paths);
    assert!(
        again.backdrop_luminance() > 0.5,
        "the next window opens light"
    );
}
