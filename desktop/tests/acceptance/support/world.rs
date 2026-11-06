//! The people around the window: an organisation on a real server, its owners, a CI machine
//! that stores builds, and a person whose computer the window runs on.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use oxabl_cache_client::{
    ArtifactFileRef, ArtifactPath, Cache, CompileKey, Config, Credentials, FetchOptions,
    FetchRequest, Identity, IdentitySource, InitRequest, Invite, Mode, Owner, Role, SecretString,
    create_identity_file, init,
};
use oxabl_cache_desktop::data::paths::Paths;
use oxabl_cache_e2e::cache_server::CacheServer;
use oxabl_cache_e2e::workdir::TestDir;
use serde_json::{Value, json};

pub const ISSUER_PREFIX: &str = "/people";
pub const CLIENT_ID: &str = "oxabl-desktop";

fn secret(text: &str) -> SecretString {
    SecretString::from(text.to_owned())
}

fn json_of(output: &std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("json")
}

/// A tenant on a running server with its administrator's token.
pub struct Org {
    pub server: CacheServer,
    pub tenant: String,
    pub admin: String,
    pub issuer: String,
    pub dir: TestDir,
}

/// A cache an owner has set up, and the invite the owner sends people.
pub struct OwnedCache {
    pub id: String,
    /// The owner's identity string.
    pub owner: SecretString,
    pub owner_token: String,
    pub repo: PathBuf,
    pub invite: Invite,
}

impl Org {
    /// A server whose `[oidc]` section is `oidc`, and a tenant on it whose people sign in at
    /// `issuer`.
    pub async fn start(oidc: &str, issuer: &str) -> Org {
        oxabl_cache_e2e::cache_server::ensure_built();
        let server = CacheServer::start(oidc).await;
        let tenant = json_of(&server.admin_cli(&["--json", "tenant", "create", "acme"]))["id"]
            .as_str()
            .expect("tenant")
            .to_owned();
        let admin = json_of(&server.admin_cli(&[
            "--json",
            "token",
            "create",
            "--tenant",
            &tenant,
            "--scopes",
            "tenant:admin",
        ]))["token"]
            .as_str()
            .expect("token")
            .to_owned();
        Org {
            server,
            tenant,
            admin,
            issuer: issuer.to_owned(),
            dir: TestDir::new("org").expect("org directory"),
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.server.public)
    }

    async fn call(&self, method: &str, path: &str, body: Value) -> Value {
        let reply = self
            .server
            .api(&self.admin, method, path, Some(&body))
            .await;
        assert!(
            reply.status < 300,
            "{method} {path}: {} {}",
            reply.status,
            reply.body
        );
        if reply.body.is_empty() {
            Value::Null
        } else {
            reply.json()
        }
    }

    /// A token for `caches` with `scopes`.
    pub async fn token(&self, name: &str, scopes: &[&str], caches: &[&str]) -> String {
        self.call(
            "POST",
            "/v1/tenant/tokens",
            json!({"name": name, "scopes": scopes, "caches": caches}),
        )
        .await["token"]
            .as_str()
            .expect("token")
            .to_owned()
    }

    /// A cache created with a new owner's key, its first roster, and an invite to it.
    pub async fn cache(&self, name: &str) -> OwnedCache {
        let owner = Identity::generate().expect("owner identity");
        let owner_text = owner.to_secret_string().expect("identity string");
        let key: String = owner
            .signing_public()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let id = self
            .call(
                "POST",
                "/v1/tenant/caches",
                json!({"display_name": name, "owner_keys": [key]}),
            )
            .await["id"]
            .as_str()
            .expect("cache id")
            .to_owned();
        let owner_token = self
            .token(&format!("{name}-owner"), &["read", "cache:admin"], &[&id])
            .await;
        let state = self.dir.path().join(format!("owner-{id}"));
        let report = init(
            InitRequest::new(
                self.url().parse().expect("url"),
                &self.tenant,
                &id,
                "owner",
                owner,
            )
            .with_credentials(Credentials::Token(secret(&owner_token)))
            .with_state_dir(state),
        )
        .await
        .expect("init");
        let repo = self.dir.path().join(format!("repo-{id}"));
        fs::create_dir_all(repo.join(".oxabl")).expect("repo");
        fs::write(repo.join(".oxabl/cache.toml"), report.cache_toml).expect("cache.toml");
        let config = Config::from_repo(&repo).expect("config");
        let invite = Invite::new(&config, name, &self.issuer, CLIENT_ID).expect("invite");
        OwnedCache {
            id,
            owner: owner_text,
            owner_token,
            repo,
            invite,
        }
    }

    /// The owner adds the member the descriptor names.
    pub async fn add_member(&self, cache: &OwnedCache, descriptor: &str, label: &str, role: Role) {
        let config = Config::from_repo(&cache.repo)
            .expect("config")
            .with_credentials(Credentials::Token(secret(&cache.owner_token)))
            .with_identity(IdentitySource::Value(cache.owner.clone()))
            .with_state_dir(self.dir.path().join(format!("owner-{}", cache.id)));
        let owner = Owner::open(config)
            .await
            .expect("the owner opens the cache");
        owner
            .add_member(descriptor, label, role)
            .await
            .expect("the owner adds the member");
    }

    /// Registers the issuer, makes `subject` a user of the tenant and grants them a developer's
    /// scopes on `cache`.
    pub async fn person_signs_in_with(&self, subject: &str, cache: &str) {
        self.call(
            "POST",
            "/v1/tenant/oidc/issuers",
            json!({"issuer": self.issuer, "shape": "custom", "id_claims": ["sub"]}),
        )
        .await;
        let user = self
            .call(
                "POST",
                "/v1/tenant/users",
                json!({"issuer": self.issuer, "subject": subject, "label": subject}),
            )
            .await;
        self.call(
            "PUT",
            "/v1/tenant/grants",
            json!({"principal": user["id"], "cache": cache, "scopes": ["read", "write:personal"]}),
        )
        .await;
    }
}

/// Artifact sets on disk, one per compile key.
pub struct Build {
    pub sets: Vec<(CompileKey, Vec<ArtifactFileRef>)>,
}

/// Writes `count` artifact sets of a few kilobytes under `dir`.
pub fn build(dir: &Path, seed: u8, count: usize) -> Build {
    let mut sets = Vec::new();
    for n in 0..count {
        let name = format!("r/unit{n:03}.r");
        let disk = dir.join(format!("unit{seed}-{n:03}.r"));
        fs::create_dir_all(dir).expect("build dir");
        let bytes: Vec<u8> = (0..4_000 + n * 97)
            .map(|i| (i as u8) ^ seed ^ n as u8)
            .collect();
        fs::write(&disk, bytes).expect("artifact");
        let file =
            ArtifactFileRef::from_disk(ArtifactPath::new(&name).expect("path"), disk).expect("ref");
        let mut key = [seed; 32];
        key[..8].copy_from_slice(&(n as u64).to_be_bytes());
        sets.push((CompileKey(key), vec![file]));
    }
    Build { sets }
}

/// A machine that is not the window's: CI, or another tool on the person's computer.
pub struct Machine {
    pub cache: Cache,
}

impl Machine {
    pub async fn open(
        cache: &OwnedCache,
        token: &str,
        identity: &Identity,
        mode: Mode,
        state: PathBuf,
    ) -> Machine {
        let config = Config::from_repo(&cache.repo)
            .expect("config")
            .with_mode(mode)
            .with_credentials(Credentials::Token(secret(token)))
            .with_identity(IdentitySource::Value(
                identity.to_secret_string().expect("identity"),
            ))
            .with_state_dir(state);
        Machine {
            cache: Cache::open(config).await.expect("open"),
        }
    }

    pub async fn store(&self, build: &Build) {
        let uploader = self.cache.uploader().expect("uploader");
        for (key, files) in &build.sets {
            uploader.put(*key, files.clone()).await;
        }
        uploader.finish().await;
        self.cache.share_activity().await;
    }

    /// Fetches the build's keys and `extra` keys nobody stored.
    pub async fn fetch(&self, build: &Build, extra: usize, stage: PathBuf) {
        let mut requests: Vec<FetchRequest> = build
            .sets
            .iter()
            .map(|(key, _)| FetchRequest {
                key: *key,
                expect: None,
            })
            .collect();
        requests.extend((0..extra).map(|n| FetchRequest {
            key: CompileKey([0xEE - n as u8; 32]),
            expect: None,
        }));
        let mut fetch = self.cache.fetch(requests, FetchOptions::new(stage));
        while fetch.next().await.is_some() {}
        self.cache.share_activity().await;
    }
}

/// The person's computer: the window's directories, an identity whose passphrase is
/// `passphrase`, and a stored sign-in holding `token`.
pub struct Person {
    pub dir: TestDir,
    pub paths: Paths,
    pub passphrase: String,
}

impl Person {
    pub fn new(name: &str) -> Person {
        let dir = TestDir::new(name).expect("person directory");
        let paths = Paths::under(dir.path());
        Person {
            dir,
            paths,
            passphrase: format!("pass-{name}-{}", std::process::id()),
        }
    }

    /// An identity file on this computer, as an earlier join made it.
    pub fn make_identity(&self) -> Identity {
        create_identity_file(&self.paths.identity_file, &secret(&self.passphrase))
            .expect("identity file")
    }

    /// A sign-in session for the tenant, as `login` stores it, holding `token`.
    pub fn sign_in(&self, tenant: &str, token: &str) {
        let dir = self.paths.state_dir.join("session");
        fs::create_dir_all(&dir).expect("session dir");
        let path = dir.join(tenant);
        let expires = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_secs()
            + 3_600;
        fs::write(
            &path,
            json!({"token": token, "expires_at": expires}).to_string(),
        )
        .expect("session");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("mode");
    }

    /// The window's settings with these invites, as joining leaves them.
    pub fn joined(&self, invites: &[&Invite]) {
        let settings = oxabl_cache_desktop::data::settings::Settings {
            invites: invites.iter().map(|i| i.to_text()).collect(),
            ..Default::default()
        };
        settings.save(&self.paths.config_file).expect("settings");
    }
}
