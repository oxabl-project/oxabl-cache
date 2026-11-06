use std::collections::BTreeMap;

use oxabl_cache_client::{Invite, SecretString};
use rune::TextEdit;

use crate::data::model::CacheCard;
use crate::data::paths::Paths;
use crate::data::settings::Settings;
use crate::data::worker::Worker;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Home,
    Detail(String),
    Join,
    Settings,
    Unlock,
}

/// Whose numbers the detail screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Team,
    Mine,
}

#[derive(Debug)]
pub enum JoinStep {
    /// Step 1: paste the invite.
    Invite,
    /// Step 2: approve the sign-in in the browser.
    SignIn {
        url: Option<String>,
        code: Option<String>,
        failed: Option<String>,
    },
    /// Step 3: send the request to an owner and wait.
    Request {
        command: Option<String>,
        passphrase: Option<SecretString>,
        failed: Option<String>,
    },
    Done,
}

#[derive(Debug)]
pub struct JoinFlow {
    pub step: JoinStep,
    pub edit: TextEdit,
    pub invite: Option<Invite>,
    pub error: Option<String>,
}

impl Default for JoinFlow {
    fn default() -> JoinFlow {
        JoinFlow {
            step: JoinStep::Invite,
            edit: TextEdit::new(""),
            invite: None,
            error: None,
        }
    }
}

#[derive(Debug)]
pub struct UnlockForm {
    pub edit: TextEdit,
    pub remember: bool,
    pub error: Option<String>,
    pub busy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Speed {
    Testing,
    Measured(Option<u64>),
}

/// The whole window's state.
#[derive(Debug)]
pub struct Ui {
    pub paths: Paths,
    pub settings: Settings,
    pub cards: Vec<CacheCard>,
    pub screen: Screen,
    pub side: Side,
    pub join: JoinFlow,
    pub unlock: UnlockForm,
    pub needs_unlock: bool,
    pub refreshing: bool,
    pub refreshed_at: Option<u64>,
    pub toast: Option<(String, f64)>,
    pub speed: BTreeMap<String, Speed>,
    pub worker: Option<Worker>,
    /// Seconds since the window started, as `poll` last saw them.
    pub clock: f64,
    pub last_refresh: f64,
    /// When the effects last had a reason to move; they settle a while after.
    pub stirred: f64,
}

impl Ui {
    /// The state the window opens with: the saved settings and the cards the last run saw, so
    /// the first frame has content and no network has been asked anything yet.
    pub fn load(paths: Paths) -> Ui {
        let settings = Settings::load(&paths.config_file);
        let mut cards = crate::data::settings::load_snapshot(&paths.snapshot_file());
        for invite in settings
            .invites
            .iter()
            .filter_map(|t| Invite::parse(t).ok())
        {
            if !cards.iter().any(|c| c.id == invite.cache()) {
                cards.push(CacheCard::new(
                    &invite.cache(),
                    &invite.tenant(),
                    invite.name(),
                    invite.server().host_str().unwrap_or("cache"),
                    crate::data::model::Standing::Loading,
                ));
            }
        }
        let refreshed_at = std::fs::metadata(paths.snapshot_file())
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        Ui {
            settings,
            cards,
            screen: Screen::Home,
            side: Side::Team,
            join: JoinFlow::default(),
            unlock: UnlockForm {
                edit: TextEdit::new(""),
                remember: true,
                error: None,
                busy: false,
            },
            needs_unlock: false,
            refreshing: false,
            refreshed_at,
            toast: None,
            speed: BTreeMap::new(),
            worker: None,
            clock: 0.0,
            last_refresh: 0.0,
            stirred: 0.0,
            paths,
        }
    }

    /// Connects the background and asks it for fresh numbers.
    pub fn connect(&mut self, worker: Worker) {
        self.worker = Some(worker);
        self.refreshing = true;
        self.send(crate::data::worker::Command::Start {
            invites: self.settings.invites.clone(),
        });
    }

    pub fn send(&self, command: crate::data::worker::Command) {
        if let Some(worker) = &self.worker {
            worker.send(command);
        }
    }

    pub fn joined_cards(&self) -> impl Iterator<Item = &CacheCard> {
        self.cards.iter().filter(|c| c.is_joined())
    }

    pub fn card(&self, id: &str) -> Option<&CacheCard> {
        self.cards.iter().find(|c| c.id == id)
    }

    pub fn invite_for(&self, cache: &str) -> Option<Invite> {
        self.settings
            .invites
            .iter()
            .filter_map(|text| Invite::parse(text).ok())
            .find(|invite| invite.cache() == cache)
    }

    pub fn say(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), self.clock + 3.5));
    }

    pub fn save_settings(&mut self) {
        if self.settings.save(&self.paths.config_file).is_err() {
            self.say("Couldn't save the settings");
        }
    }
}
