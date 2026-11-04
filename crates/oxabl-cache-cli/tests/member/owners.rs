//! Owners: a new owner cosigns the roster that adds it, the departing owner's key drops out of
//! it with a new epoch, and a roster never loses its last owner or its signer.

use oxabl_cache_client::{Mode, OpenError};

use crate::support::cache::Member;
use crate::support::person::Person;
use crate::support::world::World;

struct Pair {
    world: World,
    cache: String,
    alice: Person,
    alice_key: String,
    alice_hex: String,
    bob: Person,
    bob_descriptor: String,
    bob_key: String,
}

fn toml(world: &World, cache: &str, keys: &[&str], min: u64) -> String {
    let keys: Vec<String> = keys.iter().map(|k| format!("\"{k}\"")).collect();
    format!(
        "server = \"{}\"\ntenant = \"{}\"\ncache = \"{cache}\"\nowner_keys = [{}]\nmin_roster_version = {min}\n",
        world.url(),
        world.tenant,
        keys.join(", ")
    )
}

impl Pair {
    async fn start() -> Self {
        let world = World::start().await;
        let mut alice = Person::new("alice");
        alice.use_repo(world.repo.path());
        let made = alice
            .cli(&["identity", "new", "--file", &alice.identity_arg()])
            .ok()
            .json();
        let alice_hex = made["signing_key_hex"].as_str().expect("key").to_owned();
        let alice_key = made["signing_key"].as_str().expect("key").to_owned();
        let cache = world.create_cache("codebase", &alice_hex).await;
        alice.use_token(&world.token("alice", &["read", "cache:admin"], &cache).await);
        let init = alice
            .cli(&[
                "init",
                "--server",
                &world.url(),
                "--tenant",
                &world.tenant,
                "--cache",
                &cache,
                "--label",
                "alice",
            ])
            .ok()
            .json();
        world.write_repo_file(init["cache_toml"].as_str().expect("cache.toml"));

        let mut bob = Person::new("bob");
        let bob_made = bob
            .cli(&["identity", "new", "--file", &bob.identity_arg()])
            .ok()
            .json();
        bob.use_token(&world.token("bob", &["read", "cache:admin"], &cache).await);
        Self {
            bob_descriptor: bob_made["descriptor"]
                .as_str()
                .expect("descriptor")
                .to_owned(),
            bob_key: bob_made["signing_key"].as_str().expect("key").to_owned(),
            world,
            cache,
            alice,
            alice_key,
            alice_hex,
            bob,
        }
    }

    fn bob_member(&self) -> Member {
        Member::with_file(&self.bob.token, &self.bob.identity, &self.bob.passphrase)
    }

    fn add_bob(&self) -> crate::support::cli::Output {
        self.alice.cli(&[
            "owner",
            "add",
            &self.bob_descriptor,
            "--label",
            "bob",
            "--cosign-identity-file",
            &self.bob.identity_arg(),
            "--cosign-passphrase-file",
            &self.bob.passphrase_file.display().to_string(),
        ])
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_owner_cosigns_and_the_old_owner_can_then_be_removed() {
    let pair = Pair::start().await;
    let added = pair.add_bob().ok().json();
    assert_eq!(
        (added["roster_version"].as_u64(), added["epoch"].as_u64()),
        (Some(2), Some(1))
    );
    assert_eq!(
        added["owner_keys"],
        serde_json::json!([pair.alice_key, pair.bob_key])
    );
    assert_eq!(added["owner_keys_hex"].as_array().map(Vec::len), Some(2));
    assert_eq!(added["owner_keys_hex"][0], pair.alice_hex);

    let state = pair.world.repo.path().join("state");
    let only_alice = pair
        .world
        .checkout(&toml(&pair.world, &pair.cache, &[&pair.alice_key], 0));
    let only_bob = pair
        .world
        .checkout(&toml(&pair.world, &pair.cache, &[&pair.bob_key], 0));
    let member = pair.bob_member();
    for (name, repo) in [("alice", &only_alice), ("bob", &only_bob)] {
        member
            .open(repo.path(), Mode::ReadOnly, &state.join(name))
            .await
            .unwrap_or_else(|e| panic!("a checkout pinned to {name} alone opens: {e}"));
    }

    pair.world
        .set_owner_keys(
            &pair.cache,
            &added["owner_keys_hex"]
                .as_array()
                .expect("keys")
                .iter()
                .map(|k| k.as_str().expect("hex").to_owned())
                .collect::<Vec<_>>(),
        )
        .await;
    let both = pair.world.checkout(&toml(
        &pair.world,
        &pair.cache,
        &[&pair.alice_key, &pair.bob_key],
        2,
    ));
    let mut pair = pair;
    pair.bob.use_repo(both.path());
    let removed = pair
        .bob
        .cli(&["owner", "remove", &pair.alice_key])
        .ok()
        .json();
    assert_eq!(
        (
            removed["roster_version"].as_u64(),
            removed["epoch"].as_u64()
        ),
        (Some(3), Some(2))
    );
    assert_eq!(removed["owner_keys"], serde_json::json!([pair.bob_key]));
    assert_eq!(removed["min_roster_version"], 3);

    assert!(
        matches!(
            member
                .open(only_alice.path(), Mode::ReadOnly, &state.join("alice-2"))
                .await,
            Err(OpenError::Roster(_))
        ),
        "a checkout that pins only the removed key refuses the new roster"
    );
    member
        .open(only_bob.path(), Mode::ReadOnly, &state.join("bob-2"))
        .await
        .expect("a checkout that pins the remaining key opens");
    pair.alice
        .cli(&[
            "member",
            "add",
            &pair.bob_descriptor,
            "--label",
            "x",
            "--role",
            "reader",
        ])
        .exits(1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_owner_add_without_the_new_owners_own_signature_is_refused_and_publishes_nothing() {
    let pair = Pair::start().await;
    let stranger = Person::new("carol");
    stranger
        .cli(&["identity", "new", "--file", &stranger.identity_arg()])
        .ok();
    let refused = pair.alice.cli(&[
        "owner",
        "add",
        &pair.bob_descriptor,
        "--label",
        "bob",
        "--cosign-identity-file",
        &stranger.identity_arg(),
        "--cosign-passphrase-file",
        &stranger.passphrase_file.display().to_string(),
    ]);
    refused.exits(2);
    pair.alice
        .cli(&["owner", "add", &pair.bob_descriptor, "--label", "bob"])
        .exits(2);
    let status = pair.alice.cli(&["status"]).ok().json();
    assert_eq!(status["roster"]["version"], 1, "nothing was published");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_signing_owner_cannot_remove_itself_and_only_owners_leave_through_owner_remove() {
    let pair = Pair::start().await;
    pair.add_bob().ok();
    pair.alice.cli(&["owner", "remove", "alice"]).exits(1);
    pair.alice
        .cli(&["owner", "remove", &pair.alice_key])
        .exits(1);
    pair.alice.cli(&["owner", "remove", "nobody"]).exits(1);
    pair.alice.cli(&["member", "remove", "bob"]).exits(2);
    let carol = Person::new("carol");
    let carol_made = carol
        .cli(&["identity", "new", "--file", &carol.identity_arg()])
        .ok()
        .json();
    pair.alice
        .cli(&[
            "member",
            "add",
            carol_made["descriptor"].as_str().expect("descriptor"),
            "--label",
            "carol",
            "--role",
            "developer",
        ])
        .ok();
    pair.alice.cli(&["owner", "remove", "carol"]).exits(2);
    let status = pair.alice.cli(&["status"]).ok().json();
    assert_eq!(
        status["roster"]["version"], 3,
        "every refused change left the roster as it was"
    );
    assert_eq!(status["roster"]["epoch"], 1);
}
