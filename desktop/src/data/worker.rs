use std::collections::BTreeMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use oxabl_cache_client::{Invite, SecretString};
use tokio::sync::mpsc as async_mpsc;
use tokio::task::JoinHandle;

use super::model::CacheCard;
use super::paths::Paths;
use super::{identity, join, refresh};

/// What the window asks of the background.
#[derive(Debug)]
pub enum Command {
    /// Unlock the identity from the keyring if it can, then refresh.
    Start {
        invites: Vec<String>,
    },
    Refresh {
        invites: Vec<String>,
    },
    Unlock {
        passphrase: SecretString,
        remember: bool,
        invites: Vec<String>,
    },
    /// Find out what joining still needs: a sign-in, a token, or nothing.
    Prepare {
        invite: Invite,
    },
    SignIn {
        invite: Invite,
    },
    /// Check a pasted access token and keep it.
    UseToken {
        invite: Invite,
        token: SecretString,
    },
    Join {
        invite: Invite,
    },
    CancelJoin,
    CopyPassphrase,
    SignOut {
        invite: Invite,
        invites: Vec<String>,
    },
    SpeedTest {
        invite: Invite,
    },
}

/// What the background tells the window.
#[derive(Debug)]
pub enum Event {
    Card(Box<CacheCard>),
    /// Every card of a refresh has been sent; caches not named are gone.
    Refreshed {
        at: u64,
        ids: Vec<String>,
    },
    NeedsUnlock,
    Unlocked,
    UnlockFailed(String),
    SignInCode {
        url: String,
        code: String,
    },
    /// This machine has a credential for the organisation: a sign-in or an accepted token.
    SignedIn,
    SignInFailed(String),
    NeedsSignIn,
    /// The organisation takes an access token; with why the last one did not do.
    NeedsToken(Option<String>),
    TokenRefused(String),
    /// What the person sends an owner, and the passphrase of an identity made just now.
    JoinRequest {
        command: String,
        new_passphrase: Option<SecretString>,
    },
    Joined {
        cache: String,
    },
    JoinFailed(String),
    Passphrase(SecretString),
    Notice(String),
    Speed {
        cache: String,
        bytes_per_second: Option<u64>,
    },
}

/// Shared by every task of the background.
pub(crate) struct Context {
    pub paths: Paths,
    pub events: mpsc::Sender<Event>,
    pub wake: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The unlocked identity string, once unlocked.
    pub identity: Mutex<Option<SecretString>>,
    /// Pasted tokens by organisation and server; `None` when the keyring has none.
    pub tokens: Mutex<BTreeMap<String, Option<SecretString>>>,
}

impl Context {
    pub fn send(&self, event: Event) {
        if self.events.send(event).is_ok()
            && let Some(wake) = &self.wake
        {
            wake();
        }
    }

    pub fn identity(&self) -> Option<SecretString> {
        self.identity.lock().expect("identity lock").clone()
    }
}

/// The background: a small runtime on its own thread, fed by commands, answering with events.
pub struct Worker {
    commands: async_mpsc::UnboundedSender<Command>,
    events: mpsc::Receiver<Event>,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Worker")
    }
}

impl Worker {
    /// Starts the background; `wake` is called after every event so the window polls.
    pub fn start(paths: Paths, wake: Option<Arc<dyn Fn() + Send + Sync>>) -> Worker {
        let (commands, mut inbox) = async_mpsc::unbounded_channel();
        let (events, outbox) = mpsc::channel();
        let context = Arc::new(Context {
            paths,
            events,
            wake,
            identity: Mutex::new(None),
            tokens: Mutex::new(BTreeMap::new()),
        });
        std::thread::Builder::new()
            .name("oxabl-cache-worker".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("a runtime for the background");
                runtime.block_on(async move {
                    let mut joining: Option<JoinHandle<()>> = None;
                    while let Some(command) = inbox.recv().await {
                        handle(&context, command, &mut joining);
                    }
                });
            })
            .expect("the background thread starts");
        Worker {
            commands,
            events: outbox,
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// Every event that arrived since the last call.
    pub fn drain(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

/// One step of joining runs at a time; a new one ends the last.
fn replace(joining: &mut Option<JoinHandle<()>>, next: JoinHandle<()>) {
    if let Some(previous) = joining.replace(next) {
        previous.abort();
    }
}

fn handle(context: &Arc<Context>, command: Command, joining: &mut Option<JoinHandle<()>>) {
    let context = context.clone();
    match command {
        Command::Start { invites } => {
            tokio::spawn(async move {
                identity::unlock_from_keyring(&context).await;
                refresh::refresh_all(&context, &invites).await;
            });
        }
        Command::Refresh { invites } => {
            tokio::spawn(async move { refresh::refresh_all(&context, &invites).await });
        }
        Command::Unlock {
            passphrase,
            remember,
            invites,
        } => {
            tokio::spawn(async move {
                if identity::unlock(&context, passphrase, remember).await {
                    refresh::refresh_all(&context, &invites).await;
                }
            });
        }
        Command::Prepare { invite } => {
            replace(
                joining,
                tokio::spawn(async move { join::prepare(&context, &invite).await }),
            );
        }
        Command::SignIn { invite } => {
            replace(
                joining,
                tokio::spawn(async move { join::sign_in(&context, &invite).await }),
            );
        }
        Command::UseToken { invite, token } => {
            replace(
                joining,
                tokio::spawn(async move { join::use_token(&context, &invite, token).await }),
            );
        }
        Command::Join { invite } => {
            replace(
                joining,
                tokio::spawn(async move { join::join(&context, &invite).await }),
            );
        }
        Command::CancelJoin => {
            if let Some(previous) = joining.take() {
                previous.abort();
            }
        }
        Command::CopyPassphrase => {
            tokio::spawn(async move { identity::send_passphrase(&context).await });
        }
        Command::SignOut { invite, invites } => {
            tokio::spawn(async move {
                join::sign_out(&context, &invite).await;
                refresh::refresh_all(&context, &invites).await;
            });
        }
        Command::SpeedTest { invite } => {
            tokio::spawn(async move { refresh::speed_test(&context, &invite).await });
        }
    }
}
