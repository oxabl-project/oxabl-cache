//! What the window shows for a joined cache after builds: the team's numbers, this computer's,
//! the size, the round trip, and a speed test.

use oxabl_cache_client::{Identity, Mode, Role};
use oxabl_cache_e2e::reexec;
use oxabl_cache_e2e::secret_service::SecretService;

use crate::support::window::Window;
use crate::support::world::{Machine, Org, Person, build};

#[test]
fn the_team_and_this_computer_see_their_hit_rates_pull_times_and_size() {
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
    let ci = Identity::generate().expect("ci identity");
    let ci_token = runtime.block_on(org.token("ci", &["read", "write:trusted"], &[&cache.id]));
    runtime.block_on(org.add_member(
        &cache,
        &ci.descriptor_string().expect("descriptor"),
        "ci",
        Role::Ci,
    ));

    let person = Person::new("person");
    let identity = person.make_identity();
    oxabl_cache_client::save_passphrase(
        &person.paths.identity_file,
        &oxabl_cache_client::SecretString::from(person.passphrase.clone()),
    )
    .expect("the keyring keeps the passphrase");
    let token = runtime.block_on(org.token("alice", &["read", "write:personal"], &[&cache.id]));
    person.sign_in(&org.tenant, &token);
    runtime.block_on(org.add_member(
        &cache,
        &identity.descriptor_string().expect("descriptor"),
        "alice",
        Role::Developer,
    ));
    person.joined(&[&cache.invite]);

    let work = person.dir.path().join("work");
    let built = build(&work.join("ci"), 1, 10);
    runtime.block_on(async {
        let machine = Machine::open(&cache, &ci_token, &ci, Mode::Ci, work.join("ci-state")).await;
        machine.store(&built).await;
        let laptop = Machine::open(
            &cache,
            &token,
            &identity,
            Mode::Developer,
            person.paths.state_dir.clone(),
        )
        .await;
        laptop.fetch(&built, 2, work.join("stage")).await;
    });

    let mut window = Window::open(&person.paths);
    window.wait_until("the cache's hit rate", |w| w.shows("83% hits"));
    assert_eq!(
        window.text_of("hero-hits-value").as_deref(),
        Some("83%"),
        "10 of 12 keys hit"
    );
    assert!(
        window
            .texts()
            .iter()
            .any(|t| t.contains(" of ") && t.contains("B")),
        "a size: {:?}",
        window.texts()
    );

    window.press(&format!("card-{}", cache.id));
    window.wait_for("ERP codebase");
    assert_eq!(window.text_of("stat-hits-value").as_deref(), Some("83%"));
    assert!(window.shows("over 1 pulls"), "{:?}", window.texts());
    assert!(window.shows("over 1 pushes"), "{:?}", window.texts());
    assert!(
        window
            .text_of("stat-latency-value")
            .is_some_and(|t| t.ends_with(" ms"))
    );
    assert!(window.has("chart"), "the recent pulls are drawn");

    window.press("side-mine");
    assert_eq!(window.text_of("stat-hits-value").as_deref(), Some("83%"));
    assert!(
        window.shows("over 0 pushes"),
        "this computer pushed nothing: {:?}",
        window.texts()
    );

    window.press("speed-test");
    window.wait_for("Downloaded at");

    window.key(rune::Key::Escape);
    window.wait_for("Team hit rate".to_uppercase().as_str());
}
