//! The window against a deployed server, run by hand:
//!
//! ```sh
//! OXABL_DESKTOP_LIVE_REPO=<a repository whose .oxabl/cache.toml pins the cache> \
//! OXABL_CACHE_TOKEN=<a read token> [OXABL_CACHE_IDENTITY=<a member's identity string>] \
//!   cargo test --manifest-path desktop/Cargo.toml --test acceptance live -- --ignored
//! ```
//!
//! With a member's identity the cache opens, with its size and a speed test. Without one, this
//! computer is not in the roster: the window shows the round trip and says it waits for an owner.
//!
//! `OXABL_DESKTOP_LIVE_KEEP=<dir>` keeps the state the window saved there, so a headless render
//! of the real numbers can follow.

use oxabl_cache_client::{Config, Invite};

use crate::support::window::Window;
use crate::support::world::Person;

fn required(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| panic!("{name} must be set for the live test"))
}

#[test]
#[ignore = "needs a deployed server and credentials; run by hand"]
fn a_deployed_cache_opens_with_its_round_trip_and_size() {
    let repo = required("OXABL_DESKTOP_LIVE_REPO");
    required("OXABL_CACHE_TOKEN");
    let member = std::env::var("OXABL_CACHE_IDENTITY").is_ok_and(|v| !v.is_empty());
    let config =
        Config::from_repo(std::path::Path::new(&repo)).expect("the repository's cache.toml");
    let invite = Invite::new(&config, "Live cache", None).expect("invite");
    let person = Person::new("live");
    person.joined(&[&invite]);

    let mut window = Window::open(&person.paths);
    window.wait_until("the refresh to finish", |w| w.shows("Updated "));
    eprintln!("the window shows: {:?}", window.texts());
    let card = Window::card_key(&invite.cache());
    assert!(window.has(&card), "the live cache is listed");
    if member {
        assert!(
            window.has(&format!("{}-size", invite.cache())),
            "its size: {:?}",
            window.texts()
        );
        assert!(
            window.texts().iter().any(|t| t.ends_with(" ms")),
            "{:?}",
            window.texts()
        );
        window.press(&card);
        window.press("speed-test");
        window.wait_until("a speed test result", |w| w.has("speed-result"));
        eprintln!("speed test: {:?}", window.text_of("speed-result"));
        window.key(rune::Key::Escape);
    } else {
        assert!(
            window.shows("Waiting for an owner to add this computer"),
            "{:?}",
            window.texts()
        );
    }

    if let Ok(keep) = std::env::var("OXABL_DESKTOP_LIVE_KEEP") {
        let keep = std::path::Path::new(&keep);
        std::fs::create_dir_all(keep.join("state/oxabl-cache")).expect("keep");
        std::fs::create_dir_all(keep.join("config/oxabl-cache")).expect("keep");
        std::fs::copy(
            person.paths.snapshot_file(),
            keep.join("state/oxabl-cache/desktop-snapshot.json"),
        )
        .expect("snapshot");
        std::fs::copy(
            &person.paths.config_file,
            keep.join("config/oxabl-cache/desktop.toml"),
        )
        .expect("settings");
    }
}
