//! A tenant, a cache and its principals on the real SQLite and filesystem adapters.

use super::assembler::TestAssembler;
use super::clock::TestClock;
use super::flaky::FlakyMeta;
use super::keys::Keys;
use super::scratch::Scratch;
use oxabl_cache_core::domain::principal::Caller;
use oxabl_cache_core::domain::scope::{Grant, Scope, ScopeSet};
use oxabl_cache_core::ids::{CacheId, TenantId};
use oxabl_cache_core::usecases::{CacheSpec, TenantSpec};
use oxabl_cache_core::{Config, Engine};
use oxabl_cache_meta_sqlite::SqliteStore;
use oxabl_cache_store_fs::FsStore;
use std::path::Path;
use std::sync::Arc;

pub type TestEngine = Engine<FlakyMeta<SqliteStore>, FsStore, TestAssembler, TestClock>;

pub const URL_KEY: [u8; 32] = [7; 32];

/// Opens an engine on the files under `dir`; reopening the same `dir` is a process restart.
pub fn open_engine(
    dir: &Path,
    clock: &TestClock,
    config: Config,
) -> (TestEngine, FlakyMeta<SqliteStore>) {
    let meta = FlakyMeta::new(SqliteStore::open(&dir.join("meta.db")).expect("open metadata"));
    let store =
        FsStore::open(dir.join("objects"), "http://cache.test", vec![URL_KEY]).expect("open store");
    (
        Engine::new(meta.clone(), store, TestAssembler, clock.clone(), config),
        meta,
    )
}

pub struct Env {
    pub engine: Arc<TestEngine>,
    pub meta: FlakyMeta<SqliteStore>,
    pub clock: TestClock,
    pub dir: Scratch,
    pub tenant: TenantId,
    pub admin: Caller,
    pub cache: CacheId,
    pub keys: Keys,
    pub ci: Caller,
    pub dev: Caller,
    pub dev2: Caller,
    pub reader: Caller,
    pub cache_admin: Caller,
}

pub fn small_config() -> Config {
    Config::default()
}

impl Env {
    pub async fn new() -> Self {
        Self::with(small_config(), 1 << 30).await
    }

    pub async fn with(config: Config, hard_bytes: u64) -> Self {
        Self::build(config, hard_bytes, None).await
    }

    pub async fn with_egress(egress_bytes: u64) -> Self {
        Self::build(small_config(), 1 << 30, Some(egress_bytes)).await
    }

    pub fn object_path(&self, pack: oxabl_cache_core::ids::PackId) -> std::path::PathBuf {
        let name = pack.to_string();
        self.dir
            .path()
            .join("objects/v1")
            .join(self.cache.to_string())
            .join(&name[..2])
            .join(name)
    }

    async fn build(config: Config, hard_bytes: u64, egress_bytes: Option<u64>) -> Self {
        let dir = Scratch::new();
        let clock = TestClock::new();
        let (engine, meta) = open_engine(dir.path(), &clock, config);
        let engine = Arc::new(engine);
        let tenant = engine
            .create_tenant(TenantSpec {
                name: "acme".into(),
                hard_bytes: None,
                egress_bytes,
                rate: 50,
                burst: 200,
            })
            .await
            .expect("create tenant");
        let admin_user = Caller {
            tenant,
            principal: oxabl_cache_core::ids::PrincipalId::random(),
            grants: vec![Grant {
                cache: None,
                scopes: ScopeSet::EMPTY.with(Scope::TenantAdmin),
            }],
        };
        let keys = Keys::new();
        let cache = engine
            .create_cache(
                &admin_user,
                CacheSpec {
                    display_name: "codebase".into(),
                    retention_days: None,
                    hard_bytes: Some(hard_bytes),
                    owner_keys: vec![keys.owner_public()],
                },
            )
            .await
            .expect("create cache");
        let mut env = Self {
            engine,
            meta,
            clock,
            dir,
            tenant,
            admin: admin_user.clone(),
            cache,
            keys,
            ci: admin_user.clone(),
            dev: admin_user.clone(),
            dev2: admin_user.clone(),
            reader: admin_user.clone(),
            cache_admin: admin_user,
        };
        env.ci = env.workload(&[Scope::Read, Scope::WriteTrusted]).await;
        env.dev = env.user("dev", &[Scope::Read, Scope::WritePersonal]).await;
        env.dev2 = env.user("dev2", &[Scope::Read, Scope::WritePersonal]).await;
        env.reader = env.user("reader", &[Scope::Read]).await;
        env.cache_admin = env
            .user("cache-admin", &[Scope::Read, Scope::CacheAdmin])
            .await;
        env
    }

    pub async fn user(&self, subject: &str, scopes: &[Scope]) -> Caller {
        let id = self
            .engine
            .create_user(
                &self.admin,
                "https://idp.test".into(),
                subject.into(),
                subject.into(),
            )
            .await
            .expect("create user");
        self.engine
            .set_grant(
                &self.admin,
                id,
                Grant {
                    cache: Some(self.cache),
                    scopes: scopes.iter().copied().collect(),
                },
            )
            .await
            .expect("grant");
        let session = self
            .engine
            .open_user_session(self.tenant, id)
            .await
            .expect("session");
        self.engine
            .authenticate(&session.secret)
            .await
            .expect("authenticate")
    }

    pub async fn workload(&self, scopes: &[Scope]) -> Caller {
        let session = self
            .engine
            .open_workload_session(
                self.tenant,
                oxabl_cache_core::ids::PrincipalId::random(),
                vec![Grant {
                    cache: Some(self.cache),
                    scopes: scopes.iter().copied().collect(),
                }],
            )
            .await
            .expect("workload session");
        self.engine
            .authenticate(&session.secret)
            .await
            .expect("authenticate")
    }

    pub async fn object_exists(&self, pack: oxabl_cache_core::ids::PackId) -> bool {
        use oxabl_cache_core::ports::{ObjectKey, ObjectStore};
        self.engine
            .object_store()
            .head(&ObjectKey {
                cache: self.cache,
                pack,
            })
            .await
            .expect("head")
            .is_some()
    }

    pub async fn info(&self) -> oxabl_cache_core::usecases::CacheInfo {
        self.engine
            .cache_info(&self.reader, self.cache)
            .await
            .expect("cache info")
    }

    pub async fn assert_consistent(&self) {
        let problems = self.engine.audit_cache(self.cache).await.expect("audit");
        assert!(
            problems.is_empty(),
            "byte accounting is inconsistent: {problems:?}"
        );
    }
}
