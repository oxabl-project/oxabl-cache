//! An owner prints a one-line invite for the repository's cache.

use oxabl_cache_client::Invite;

use crate::support::scenario::Owned;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_owner_prints_an_invite_that_pins_the_repository_cache() {
    let owned = Owned::start().await;
    let printed = owned
        .owner
        .text(&[
            "invite",
            "--name",
            "Codebase",
            "--issuer",
            "https://login.example.com",
            "--client-id",
            "oxabl-desktop",
        ])
        .ok()
        .stdout;
    let invite = Invite::parse(printed.trim()).expect("an invite");
    assert_eq!(invite.cache(), owned.cache);
    assert_eq!(invite.name(), "Codebase");
    assert_eq!(invite.client_id(), "oxabl-desktop");
    assert_eq!(
        invite.cache_toml(),
        std::fs::read_to_string(owned.world.repo.path().join(".oxabl/cache.toml"))
            .expect("cache.toml")
    );
    assert!(!printed.contains(&owned.owner.token));

    let json = owned
        .owner
        .cli(&[
            "invite",
            "--name",
            "Codebase",
            "--issuer",
            "https://login.example.com",
            "--client-id",
            "oxabl-desktop",
        ])
        .ok()
        .json();
    assert_eq!(
        Invite::parse(json["invite"].as_str().expect("invite")).expect("parse"),
        invite
    );

    for args in [
        vec!["invite", "--name", "Codebase", "--client-id", "x"],
        vec![
            "invite",
            "--name",
            "Codebase",
            "--issuer",
            "http://login.example.com",
            "--client-id",
            "x",
        ],
        vec![
            "invite",
            "--name",
            "",
            "--issuer",
            "https://login.example.com",
            "--client-id",
            "x",
        ],
    ] {
        owned.owner.cli(&args).exits(2);
    }
}
