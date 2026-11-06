//! Joining with an access token from the organisation's administrator, for organisations
//! without a sign-in: the window asks for the token, checks it, and keeps it in the keyring.

use std::path::Path;

use oxabl_cache_client::{ExposeSecret, Role, stored_token};
use oxabl_cache_e2e::reexec;
use oxabl_cache_e2e::secret_service::SecretService;

use crate::support::window::Window;
use crate::support::world::{Org, Person};

/// Runs the rest of the test with a private Secret Service; false in the process that only
/// started it.
fn with_keyring() -> (Option<SecretService>, bool) {
    let service = (!reexec::inside()).then(|| SecretService::start().expect("a secret service"));
    let environment = service
        .as_ref()
        .map(SecretService::environment)
        .unwrap_or_default();
    let inside = reexec::with_environment(&environment);
    (service, inside)
}

/// Every file under `dir` whose bytes contain `secret`.
fn files_holding(dir: &Path, secret: &str) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_holding(&path, secret));
        } else if std::fs::read(&path)
            .is_ok_and(|bytes| bytes.windows(secret.len()).any(|w| w == secret.as_bytes()))
        {
            found.push(path.display().to_string());
        }
    }
    found
}

#[test]
fn a_person_joins_with_an_access_token_when_the_invite_names_no_sign_in() {
    let (_service, inside) = with_keyring();
    if !inside {
        return;
    }
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let org = runtime.block_on(Org::start("", "https://login.example.com"));
    let cache = runtime.block_on(org.cache("ERP codebase"));
    let other = runtime.block_on(org.cache("Warehouse"));
    let elsewhere = runtime.block_on(org.token("bob", &["read", "write:personal"], &[&other.id]));
    let token = runtime.block_on(org.token("alice", &["read", "write:personal"], &[&cache.id]));
    let person = Person::new("person");

    let mut window = Window::open(&person.paths);
    window.wait_for("Your team's build cache");
    window.press("join");
    window.paste_into("invite-field", &cache.token_invite.to_text());
    window.press("continue");
    window.wait_for("Paste an access token");
    assert!(!window.has("sign-in-instead"), "{:?}", window.texts());

    window.press("token-continue");
    assert!(window.shows("Paste the access token you were given"));

    window.paste_into("token-field", "oxc_not_a_token");
    window.press("token-continue");
    window.wait_for("doesn't accept that token");
    assert!(stored_token(cache.invite.server(), &org.tenant).is_none());

    window.clipboard.set_text(&format!("  {elsewhere}\n"));
    window.press("paste-token");
    window.press("token-continue");
    window.wait_for("That token has no access to ERP codebase");
    assert!(stored_token(cache.invite.server(), &org.tenant).is_none());

    window.paste_into("token-field", &token);
    window.press("token-continue");
    window.wait_for("Ask to be added");
    window.wait_until("the request to send", |w| w.has("request-text"));
    assert_eq!(
        stored_token(cache.invite.server(), &org.tenant)
            .map(|t| t.expose_secret().to_owned())
            .as_deref(),
        Some(token.as_str()),
        "the accepted token is in the keyring"
    );
    window.press("copy-request");
    let request = window.copied().expect("the request is on the clipboard");
    let words: Vec<&str> = request.split_whitespace().collect();
    runtime.block_on(org.add_member(&cache, words[3], words[5], Role::Developer));

    window.wait_for("You're in");
    window.press("done");
    window.wait_until("the joined cache with its numbers", |w| {
        w.shows("ERP codebase") && w.texts().iter().any(|t| t.ends_with(" ms"))
    });
    assert_eq!(
        files_holding(person.dir.path(), &token),
        Vec::<String>::new(),
        "the token is in no file"
    );
    drop(window);

    let mut again = Window::open(&person.paths);
    again.wait_until("the cache, with the kept token", |w| {
        w.has(&format!("{}-size", cache.id))
    });
    again.key(rune::Key::Command(','));
    again.press(&format!("sign-out-{}", cache.id));
    again.wait_for("Forgot the access token");
    assert!(
        stored_token(cache.invite.server(), &org.tenant).is_none(),
        "signing out forgets the token"
    );
}

#[test]
fn a_person_whose_organisation_has_a_sign_in_can_use_a_token_instead() {
    let (_service, inside) = with_keyring();
    if !inside {
        return;
    }
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let org = runtime.block_on(Org::start("", "https://127.0.0.1:9"));
    let cache = runtime.block_on(org.cache("ERP codebase"));
    let token = runtime.block_on(org.token("alice", &["read", "write:personal"], &[&cache.id]));
    let person = Person::new("person");

    let mut window = Window::open(&person.paths);
    window.wait_for("Your team's build cache");
    window.press("join");
    window.paste_into("invite-field", &cache.invite.to_text());
    window.press("continue");
    window.wait_until("the sign-in step", |w| w.has("use-token"));
    assert!(window.shows("Sign in"), "{:?}", window.texts());
    window.press("use-token");
    window.wait_for("Paste an access token");
    assert!(window.has("sign-in-instead"));

    window.paste_into("token-field", &token);
    window.press("token-continue");
    window.wait_until("the request to send", |w| w.has("request-text"));
    assert!(window.shows("Waiting for an owner"), "{:?}", window.texts());
}
