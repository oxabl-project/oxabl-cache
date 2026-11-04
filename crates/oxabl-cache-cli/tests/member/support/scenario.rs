//! A cache an owner has initialised, ready for members.

use super::cache::Member;
use super::person::Person;
use super::world::World;

pub struct Owned {
    pub world: World,
    pub cache: String,
    pub owner: Person,
}

impl Owned {
    /// A tenant, a cache created with the owner's key, and its first roster.
    pub async fn start() -> Self {
        let world = World::start().await;
        let mut owner = Person::new("owner");
        owner.use_repo(world.repo.path());
        let made = owner
            .cli(&["identity", "new", "--file", &owner.identity_arg()])
            .ok()
            .json();
        let cache = world
            .create_cache("codebase", made["signing_key_hex"].as_str().expect("key"))
            .await;
        owner.use_token(&world.token("owner", &["read", "cache:admin"], &cache).await);
        let init = owner
            .cli(&[
                "init",
                "--server",
                &world.url(),
                "--tenant",
                &world.tenant,
                "--cache",
                &cache,
                "--label",
                "owner",
            ])
            .ok()
            .json();
        world.write_repo_file(init["cache_toml"].as_str().expect("cache.toml"));
        Self {
            world,
            cache,
            owner,
        }
    }

    /// A member with an identity file of their own, a token with `scopes` and a place in the
    /// roster as `role`.
    pub async fn member(&self, name: &str, role: &str, scopes: &[&str]) -> Person {
        let mut person = Person::new(name);
        person.use_repo(self.world.repo.path());
        let made = person
            .cli(&["identity", "new", "--file", &person.identity_arg()])
            .ok()
            .json();
        person.use_token(&self.world.token(name, scopes, &self.cache).await);
        self.owner
            .cli(&[
                "member",
                "add",
                made["descriptor"].as_str().expect("descriptor"),
                "--label",
                name,
                "--role",
                role,
            ])
            .ok();
        person
    }
}

impl Person {
    /// This person as the client library opens the cache.
    pub fn as_member(&self) -> Member {
        Member::with_file(&self.token, &self.identity, &self.passphrase)
    }
}
