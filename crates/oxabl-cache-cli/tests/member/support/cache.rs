//! The client library as oxabl uses it, opened with a token and an identity string.

use std::path::Path;

use oxabl_cache_client::{
    Cache, Config, Credentials, FetchOptions, FetchOutcome, FetchRequest, IdentitySource, Mode,
    OpenError, SecretString, StoreOutcome,
};

use super::sets::Set;

/// How a member of the cache reaches it: a token and an identity.
pub struct Member {
    pub token: String,
    pub identity: IdentitySource,
    pub passphrase: Option<String>,
}

impl Member {
    /// A CI member whose identity string is a secret of the job.
    pub fn with_secret(token: &str, identity: &str) -> Self {
        Self {
            token: token.to_owned(),
            identity: IdentitySource::Value(SecretString::from(identity.to_owned())),
            passphrase: None,
        }
    }

    /// A person whose identity is a passphrase-protected file.
    pub fn with_file(token: &str, file: &Path, passphrase: &str) -> Self {
        Self {
            token: token.to_owned(),
            identity: IdentitySource::File(file.to_path_buf()),
            passphrase: Some(passphrase.to_owned()),
        }
    }

    pub fn config(&self, repo: &Path, mode: Mode, state: &Path) -> Config {
        let config = Config::from_repo(repo)
            .expect("config")
            .with_mode(mode)
            .with_credentials(Credentials::Token(SecretString::from(self.token.clone())))
            .with_identity(self.identity.clone())
            .with_state_dir(state.to_path_buf());
        match self.passphrase.clone() {
            Some(passphrase) => {
                config.with_passphrase(move || Some(SecretString::from(passphrase.clone())))
            }
            None => config,
        }
    }

    pub async fn open(&self, repo: &Path, mode: Mode, state: &Path) -> Result<Cache, OpenError> {
        Cache::open(self.config(repo, mode, state)).await
    }
}

/// Stores the sets and returns the bytes the client uploaded.
pub async fn store(cache: &Cache, sets: &[Set]) -> u64 {
    let uploader = cache.uploader().expect("uploader");
    for set in sets {
        uploader.put(set.key, set.refs.clone()).await;
    }
    let report = uploader.finish().await;
    for (key, outcome) in &report.outcomes {
        assert!(
            matches!(outcome, StoreOutcome::Committed | StoreOutcome::Existed),
            "{key:?} was not stored: {outcome:?}"
        );
    }
    assert_eq!(report.outcomes.len(), sets.len());
    report.bytes_uploaded
}

pub async fn fetch(cache: &Cache, sets: &[Set], stage: &Path) -> Vec<FetchOutcome> {
    let requests = sets
        .iter()
        .map(|s| FetchRequest {
            key: s.key,
            expect: None,
        })
        .collect();
    let mut fetch = cache.fetch(requests, FetchOptions::new(stage.to_path_buf()));
    let mut outcomes = Vec::new();
    while let Some(outcome) = fetch.next().await {
        outcomes.push(outcome);
    }
    outcomes
}

/// Asserts that every set installed with exactly the bytes that were stored.
pub fn assert_installed(outcomes: &[FetchOutcome], sets: &[Set], stage: &Path) {
    assert_eq!(outcomes.len(), sets.len());
    for outcome in outcomes {
        assert!(
            matches!(outcome, FetchOutcome::Installed { .. }),
            "{outcome:?}"
        );
    }
    for set in sets {
        for (path, content) in &set.files {
            assert_eq!(
                std::fs::read(stage.join(path)).expect("installed file"),
                *content,
                "{path}"
            );
        }
    }
}

/// Asserts that no set was found.
pub fn assert_all_missed(outcomes: &[FetchOutcome]) {
    for outcome in outcomes {
        assert!(matches!(outcome, FetchOutcome::Miss { .. }), "{outcome:?}");
    }
}
