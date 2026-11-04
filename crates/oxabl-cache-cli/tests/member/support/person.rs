//! A person at a machine of their own: a home directory, a passphrase file, an identity.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use oxabl_cache_e2e::workdir::TestDir;

use super::cli::{Output, run};

pub struct Person {
    pub dir: TestDir,
    pub passphrase: String,
    pub identity: PathBuf,
    pub passphrase_file: PathBuf,
    pub state: PathBuf,
    pub token: String,
    pub token_file: Option<PathBuf>,
    pub repo: Option<PathBuf>,
}

pub fn write_secret(path: &Path, text: &str) {
    fs::write(path, format!("{text}\n")).expect("secret file");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("mode");
}

impl Person {
    pub fn new(name: &str) -> Self {
        let dir = TestDir::new(name).expect("person directory");
        let passphrase = format!("pass-{name}-{}", std::process::id());
        let passphrase_file = dir.path().join("passphrase");
        write_secret(&passphrase_file, &passphrase);
        Self {
            identity: dir.path().join("identity"),
            state: dir.path().join("state"),
            passphrase,
            passphrase_file,
            token: String::new(),
            token_file: None,
            repo: None,
            dir,
        }
    }

    /// Where this person keeps a file.
    pub fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Authenticates this person's commands with `token`, held in a file.
    pub fn use_token(&mut self, token: &str) {
        let path = self.file("token");
        write_secret(&path, token);
        self.token = token.to_owned();
        self.token_file = Some(path);
    }

    /// Runs this person's commands in the repository at `repo`.
    pub fn use_repo(&mut self, repo: &Path) {
        self.repo = Some(repo.to_path_buf());
    }

    /// The identity file's path as a command-line argument.
    pub fn identity_arg(&self) -> String {
        self.identity.display().to_string()
    }

    /// The global flags that make a command act as this person.
    pub fn flags(&self) -> Vec<String> {
        let path = |p: &Path| p.display().to_string();
        let mut flags = vec![
            "--identity-file".to_owned(),
            path(&self.identity),
            "--passphrase-file".to_owned(),
            path(&self.passphrase_file),
            "--state-dir".to_owned(),
            path(&self.state),
        ];
        if let Some(token) = &self.token_file {
            flags.extend(["--token-file".to_owned(), path(token)]);
        }
        if let Some(repo) = &self.repo {
            flags.extend(["--repo".to_owned(), path(repo)]);
        }
        flags
    }

    /// Runs `args` as this person, with `--json` and their flags.
    pub fn cli(&self, args: &[&str]) -> Output {
        self.cli_with(args, &[])
    }

    pub fn cli_with(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut all: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        all.push("--json".to_owned());
        all.extend(self.flags());
        run(self.dir.path(), &all, env)
    }

    /// Like `cli`, but without `--json`.
    pub fn text(&self, args: &[&str]) -> Output {
        let mut all: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        all.extend(self.flags());
        run(self.dir.path(), &all, &[])
    }
}
