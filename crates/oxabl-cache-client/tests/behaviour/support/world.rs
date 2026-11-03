//! A tenant with a cache, a roster and one member of each role, behind the hostile proxy.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use oxabl_cache_client::{
    Cache, Config, Credentials, FetchOptions, FetchOutcome, FetchRequest, IdentitySource, Mode,
    OpenError, SecretString,
};
use oxabl_cache_crypto::{ExposeSecret, Identity, RosterPins, VerifiedRoster};
use oxabl_cache_e2e::workdir::TestDir;
use oxabl_cache_format::{CacheId, Role};

use super::fixture::Set;
use super::proxy::Proxy;
use super::server::{Server, Timers, reserve_port};
use super::setup::{Setup, genesis, member};

pub struct Member {
    pub label: String,
    pub identity: String,
    pub token: String,
    pub public: [u8; 32],
}

pub struct World {
    pub server: Server,
    pub proxy: Proxy,
    pub setup: Setup,
    pub tenant: String,
    pub tenant_admin: String,
    pub cache: String,
    pub cache_admin: String,
    pub owner: Identity,
    pub ci: Member,
    pub dev: Member,
    pub dev2: Member,
    pub reader: Member,
    pub repo: TestDir,
    pub roster: Vec<u8>,
    pub roster_version: u64,
}

fn secret(text: &str) -> SecretString {
    SecretString::from(text.to_owned())
}

pub fn owner_key_text(key: &[u8; 32]) -> String {
    format!("ed25519:{}", STANDARD_NO_PAD.encode(key))
}

impl World {
    pub async fn start() -> Self {
        Self::start_with(&Timers::default()).await
    }

    pub async fn start_with(timers: &Timers) -> Self {
        Self::start_custom(timers, None).await
    }

    /// A world whose cache has a hard quota of `bytes`.
    pub async fn start_with_quota(bytes: u64) -> Self {
        Self::start_custom(&Timers::default(), Some(bytes)).await
    }

    async fn start_custom(timers: &Timers, hard_bytes: Option<u64>) -> Self {
        let (listener, port) = reserve_port();
        let server = Server::start(Some(&format!("http://127.0.0.1:{port}")), timers);
        let proxy = Proxy::start(listener, &server.address);
        let setup = Setup::new(&server.address, &server.admin_address, &server.operator);
        let (tenant, tenant_admin) = setup.tenant("acme").await;
        let owner = Identity::generate().expect("owner");
        let cache = setup
            .cache_with_quota(
                &tenant_admin,
                "codebase",
                &owner.signing_public(),
                hard_bytes,
            )
            .await;
        let cache_admin = setup
            .token(&tenant_admin, "owner", &["read", "cache:admin"], &cache)
            .await;
        let mut world = Self {
            proxy,
            setup,
            tenant,
            tenant_admin,
            cache_admin,
            owner,
            ci: placeholder(),
            dev: placeholder(),
            dev2: placeholder(),
            reader: placeholder(),
            repo: TestDir::new("repo").expect("repo directory"),
            roster: Vec::new(),
            roster_version: 1,
            cache,
            server,
        };
        world.ci = world.new_member("ci", &["read", "write:trusted"]).await;
        world.dev = world.new_member("dev", &["read", "write:personal"]).await;
        world.dev2 = world.new_member("dev2", &["read", "write:personal"]).await;
        world.reader = world.new_member("reader", &["read"]).await;
        let members = vec![
            member("owner", Role::Owner, &world.owner),
            member("ci", Role::Ci, &identity_of(&world.ci)),
            member("dev", Role::Developer, &identity_of(&world.dev)),
            member("dev2", Role::Developer, &identity_of(&world.dev2)),
            member("reader", Role::Reader, &identity_of(&world.reader)),
        ];
        let (_, bytes) = genesis(&world.cache, members, &world.owner);
        world
            .setup
            .put_roster(&world.cache_admin, &world.cache, 1, bytes.clone())
            .await;
        world.roster = bytes;
        world.write_repo_file(0);
        world
    }

    /// A member identity and a token with `scopes`.
    pub async fn new_member(&self, label: &str, scopes: &[&str]) -> Member {
        let identity = Identity::generate().expect("identity");
        let token = self
            .setup
            .token(&self.tenant_admin, label, scopes, &self.cache)
            .await;
        Member {
            label: label.to_owned(),
            identity: identity
                .to_secret_string()
                .expect("identity")
                .expose_secret()
                .to_owned(),
            token,
            public: identity.signing_public(),
        }
    }

    /// Writes `.oxabl/cache.toml` naming the proxy as the server.
    pub fn write_repo_file(&self, min_roster_version: u64) {
        let dir = self.repo.path().join(".oxabl");
        fs::create_dir_all(&dir).expect("directory");
        fs::write(
            dir.join("cache.toml"),
            format!(
                "server = \"{}\"\ntenant = \"{}\"\ncache = \"{}\"\nowner_keys = [\"{}\"]\nmin_roster_version = {min_roster_version}\n",
                self.proxy.url(),
                self.tenant,
                self.cache,
                owner_key_text(&self.owner.signing_public()),
            ),
        )
        .expect("cache.toml");
    }

    async fn publish_roster(&mut self, draft: oxabl_cache_crypto::RosterDraft) {
        let bytes = draft.sign(&[&self.owner]).expect("sign");
        self.setup
            .put_roster(
                &self.cache_admin,
                &self.cache,
                draft.version(),
                bytes.clone(),
            )
            .await;
        self.roster = bytes;
        self.roster_version = draft.version();
    }

    /// Publishes the next roster with `identity` added: same epoch, same root key.
    pub async fn add_member(&mut self, label: &str, role: Role, identity: &Identity) {
        let current = self.verified_roster();
        let root = current.unwrap_root_key(&self.owner).expect("root key");
        let draft = current
            .draft_add_member(&root, member(label, role, identity))
            .expect("draft");
        self.publish_roster(draft).await;
    }

    /// Publishes the next roster without `label`: a new epoch and a fresh root key.
    pub async fn remove_member(&mut self, label: &str) {
        let draft = self
            .verified_roster()
            .draft_remove_member(label)
            .expect("draft");
        self.publish_roster(draft).await;
    }

    /// The environment of a CI job that holds `member`'s token and identity.
    pub fn environment(&self, member: &Member, state: &Path) -> Vec<(String, String)> {
        vec![
            ("OXABL_CACHE_TOKEN".into(), member.token.clone()),
            ("OXABL_CACHE_IDENTITY".into(), member.identity.clone()),
            ("XDG_STATE_HOME".into(), state.display().to_string()),
        ]
    }

    pub fn repo(&self) -> &Path {
        self.repo.path()
    }

    pub fn pins(&self) -> RosterPins {
        RosterPins {
            cache_id: CacheId::from_text(&self.cache).expect("cache id"),
            owner_keys: vec![self.owner.signing_public()],
            min_version: 0,
        }
    }

    pub fn verified_roster(&self) -> VerifiedRoster {
        VerifiedRoster::verify(&self.roster, &self.pins(), 0).expect("roster")
    }

    /// The configuration of `member` in `mode`, with its own state directory.
    pub fn config(&self, member: &Member, mode: Mode, state: &Path) -> Config {
        Config::from_repo(self.repo())
            .expect("config")
            .with_mode(mode)
            .with_credentials(Credentials::Token(secret(&member.token)))
            .with_identity(IdentitySource::Value(secret(&member.identity)))
            .with_state_dir(state.to_path_buf())
    }

    pub async fn open(&self, member: &Member, mode: Mode) -> Result<Machine, OpenError> {
        let dirs = TestDir::new("machine").expect("machine directory");
        let cache = Cache::open(self.config(member, mode, &dirs.path().join("state"))).await?;
        Ok(Machine { cache, dirs })
    }

    /// Like `open`, with the configuration adjusted by `adjust`.
    pub async fn open_tuned(
        &self,
        member: &Member,
        mode: Mode,
        adjust: impl FnOnce(Config) -> Config,
    ) -> Machine {
        let dirs = TestDir::new("machine").expect("machine directory");
        let config = adjust(self.config(member, mode, &dirs.path().join("state")));
        Machine {
            cache: Cache::open(config).await.expect("open"),
            dirs,
        }
    }

    pub async fn machine(&self, member: &Member, mode: Mode) -> Machine {
        self.open(member, mode).await.expect("open")
    }
}

fn placeholder() -> Member {
    Member {
        label: String::new(),
        identity: String::new(),
        token: String::new(),
        public: [0; 32],
    }
}

pub fn identity_of(member: &Member) -> Identity {
    Identity::parse(&member.identity).expect("identity")
}

/// One client machine: a cache handle with its own directories.
pub struct Machine {
    pub cache: Cache,
    pub dirs: TestDir,
}

impl Machine {
    pub fn stage(&self) -> PathBuf {
        self.dirs.path().join("stage")
    }

    pub fn workspace(&self) -> PathBuf {
        self.dirs.path().join("workspace")
    }

    /// Writes `count` sets in this machine's workspace and stores them.
    pub async fn store(&self, sets: &[Set]) -> oxabl_cache_client::StoreReport {
        let uploader = self.cache.uploader().expect("uploader");
        for set in sets {
            uploader.put(set.key, set.refs()).await;
        }
        uploader.finish().await
    }

    pub async fn fetch(&self, sets: &[Set]) -> Vec<FetchOutcome> {
        self.fetch_with(sets, FetchOptions::new(self.stage())).await
    }

    pub async fn fetch_with(&self, sets: &[Set], options: FetchOptions) -> Vec<FetchOutcome> {
        let requests = sets
            .iter()
            .map(|s| FetchRequest {
                key: s.key,
                expect: None,
            })
            .collect();
        let mut fetch = self.cache.fetch(requests, options);
        let mut outcomes = Vec::new();
        while let Some(outcome) = fetch.next().await {
            outcomes.push(outcome);
        }
        outcomes
    }
}

/// Every file under `dir`, by path relative to it.
pub fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let relative = path.strip_prefix(root).expect("inside");
                out.insert(
                    relative.to_string_lossy().into_owned(),
                    fs::read(&path).expect("read"),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// Asserts that the stage holds exactly what `sets` stored for `keys`, and no temporary file.
pub fn assert_installed(stage: &Path, sets: &[Set]) {
    let found = tree(stage);
    for name in found.keys() {
        assert!(!name.contains(".oxc-"), "a temporary file remains: {name}");
    }
    for set in sets {
        for file in &set.files {
            assert_eq!(
                found.get(&file.path),
                Some(&file.bytes),
                "{} differs from what was stored",
                file.path
            );
        }
    }
}
