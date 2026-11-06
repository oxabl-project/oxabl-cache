//! What a press does. Each is a plain function of the state, so views attach them by name.

use std::time::Duration;

use oxabl_cache_client::{ExposeSecret, Invite, SecretString};
use rune::Tx;

use crate::data::model;
use crate::data::model::{CacheCard, Standing};
use crate::data::settings::Appearance;
use crate::data::worker::Command;

use super::state::{JoinFlow, JoinStep, Screen, Side, Speed, Ui};

const SECRET_CLEARS: Duration = Duration::from_secs(60);

pub fn refresh(ui: &mut Ui) {
    ui.refreshing = true;
    ui.last_refresh = ui.clock;
    ui.stirred = ui.clock;
    let invites = ui.settings.invites.clone();
    ui.send(Command::Refresh { invites });
}

pub fn open_detail(ui: &mut Ui, id: &str) {
    ui.side = Side::Team;
    ui.screen = Screen::Detail(id.to_owned());
}

pub fn home(ui: &mut Ui) {
    if !matches!(ui.join.step, JoinStep::Invite | JoinStep::Done) && ui.screen == Screen::Join {
        ui.send(Command::CancelJoin);
    }
    ui.join = JoinFlow::default();
    ui.screen = Screen::Home;
}

/// Back one level; at home, the window closes.
pub fn back(ui: &mut Ui, tx: &mut Tx) {
    match ui.screen {
        Screen::Home => tx.close_window(),
        Screen::Unlock if ui.needs_unlock => tx.close_window(),
        _ => home(ui),
    }
}

pub fn start_join(ui: &mut Ui) {
    ui.join = JoinFlow::default();
    ui.join.edit.focused = true;
    ui.screen = Screen::Join;
}

/// Reads the invite field; a valid invite moves to the next step that this machine still needs.
pub fn continue_join(ui: &mut Ui) {
    let text = ui.join.edit.text.trim().to_owned();
    let invite = match Invite::parse(&text) {
        Ok(invite) => invite,
        Err(_) if text.is_empty() => {
            ui.join.error = Some("Paste the invite you were sent.".into());
            return;
        }
        Err(_) => {
            ui.join.error = Some(
                "That isn't a complete invite. Copy the whole line, starting with oxc-invite:"
                    .into(),
            );
            return;
        }
    };
    if !ui
        .settings
        .invites
        .iter()
        .any(|t| Invite::parse(t).is_ok_and(|i| i.cache() == invite.cache()))
    {
        ui.settings.invites.push(invite.to_text());
        ui.save_settings();
    }
    if ui.card(&invite.cache()).is_none_or(|c| !c.is_joined()) {
        let host = invite.server().host_str().unwrap_or("cache").to_owned();
        ui.cards.retain(|c| c.id != invite.cache());
        ui.cards.push(CacheCard::new(
            &invite.cache(),
            &invite.tenant(),
            invite.name(),
            &host,
            Standing::AwaitingOwner,
        ));
    }
    ui.join.error = None;
    ui.join.invite = Some(invite.clone());
    ui.join.step = JoinStep::Checking;
    ui.send(Command::Prepare { invite });
}

pub fn begin_sign_in(ui: &mut Ui) {
    let Some(invite) = ui.join.invite.clone() else {
        return;
    };
    ui.join.step = JoinStep::SignIn {
        url: None,
        code: None,
        failed: None,
    };
    ui.send(Command::SignIn { invite });
}

/// Shows the token step, with why the last token did not do.
pub fn ask_for_token(ui: &mut Ui, failed: Option<String>) {
    if ui.join.invite.is_none() {
        return;
    }
    ui.send(Command::CancelJoin);
    ui.join.token = rune::TextEdit::new("");
    ui.join.token.focused = true;
    ui.join.step = JoinStep::Token {
        busy: false,
        failed,
    };
}

/// Sends the pasted token to be checked.
pub fn submit_token(ui: &mut Ui) {
    let Some(invite) = ui.join.invite.clone() else {
        return;
    };
    let text = ui.join.token.text.trim().to_owned();
    let JoinStep::Token { busy, failed } = &mut ui.join.step else {
        return;
    };
    if *busy {
        return;
    }
    if text.is_empty() {
        *failed = Some("Paste the access token you were given.".into());
        return;
    }
    *busy = true;
    *failed = None;
    ui.send(Command::UseToken {
        invite,
        token: SecretString::from(text),
    });
}

pub fn paste_token(tx: &mut Tx) {
    tx.read_clipboard(|ui: &mut Ui, content, _| {
        ui.join.token = rune::TextEdit::new(content.text().trim());
        if let JoinStep::Token { failed, .. } = &mut ui.join.step {
            *failed = None;
        }
    });
}

pub fn begin_request(ui: &mut Ui) {
    let Some(invite) = ui.join.invite.clone() else {
        return;
    };
    ui.join.step = JoinStep::Request {
        command: None,
        passphrase: None,
        failed: None,
    };
    ui.send(Command::Join { invite });
}

/// Starts joining a cache the signed-in person can see but has not joined here.
pub fn join_available(ui: &mut Ui, id: &str) {
    let Some(card) = ui.card(id).cloned() else {
        return;
    };
    let sibling = ui
        .settings
        .invites
        .iter()
        .filter_map(|t| Invite::parse(t).ok())
        .find(|i| i.tenant() == card.tenant);
    start_join(ui);
    if sibling.is_none() {
        ui.join.error = Some(format!("Ask an owner of {} for its invite.", card.name));
    } else {
        ui.join.error = Some(format!(
            "Paste the invite for {}. An owner can make one with oxabl-cache invite.",
            card.name
        ));
    }
}

pub fn sign_in_again(ui: &mut Ui, id: &str) {
    let Some(invite) = ui.invite_for(id) else {
        return;
    };
    start_join(ui);
    ui.join.edit = rune::TextEdit::new(invite.to_text());
    continue_join(ui);
}

pub fn finish_joining(ui: &mut Ui, id: &str) {
    sign_in_again(ui, id);
}

pub fn open_sign_in_page(ui: &mut Ui, tx: &mut Tx) {
    if let JoinStep::SignIn { url: Some(url), .. } = &ui.join.step {
        let url = url.clone();
        if tx.open_url(&url).is_err() {
            tx.copy(url);
            ui.say("Couldn't open a browser, so the link is on the clipboard");
        }
    }
}

pub fn copy_code(ui: &mut Ui, tx: &mut Tx) {
    if let JoinStep::SignIn {
        code: Some(code), ..
    } = &ui.join.step
    {
        tx.copy(code.clone());
        ui.say("Code copied");
    }
}

pub fn copy_request(ui: &mut Ui, tx: &mut Tx) {
    if let JoinStep::Request {
        command: Some(command),
        ..
    } = &ui.join.step
    {
        tx.copy(command.clone());
        ui.say("Copied. Send it to an owner of the cache");
    }
}

pub fn copy_secret(ui: &mut Ui, tx: &mut Tx, secret: &SecretString) {
    let _ = tx.copy_secret(secret.expose_secret().to_owned(), SECRET_CLEARS);
    ui.say("Passphrase copied. The clipboard clears in a minute");
}

pub fn copy_new_passphrase(ui: &mut Ui, tx: &mut Tx) {
    if let JoinStep::Request {
        passphrase: Some(passphrase),
        ..
    } = &ui.join.step
    {
        let passphrase = passphrase.clone();
        copy_secret(ui, tx, &passphrase);
    }
}

pub fn copy_stored_passphrase(ui: &mut Ui) {
    ui.send(Command::CopyPassphrase);
}

pub fn paste_invite(tx: &mut Tx) {
    tx.read_clipboard(|ui: &mut Ui, content, _| {
        ui.join.edit = rune::TextEdit::new(content.text().trim());
        ui.join.error = None;
        if Invite::parse(content.text()).is_err() {
            ui.join.error = Some("The clipboard doesn't hold an invite.".into());
        }
    });
}

pub fn unlock(ui: &mut Ui) {
    let text = ui.unlock.edit.text.trim().to_owned();
    if text.is_empty() {
        ui.unlock.error = Some("Type or paste your passphrase.".into());
        return;
    }
    ui.unlock.busy = true;
    ui.unlock.error = None;
    let invites = ui.settings.invites.clone();
    ui.send(Command::Unlock {
        passphrase: SecretString::from(text),
        remember: ui.unlock.remember,
        invites,
    });
}

pub fn set_appearance(ui: &mut Ui, tx: &mut Tx, appearance: Appearance) {
    tx.color_motion(rune::tok::motion::CALM);
    ui.settings.appearance = appearance;
    ui.save_settings();
}

pub fn toggle_effects(ui: &mut Ui) {
    ui.settings.effects = !ui.settings.effects;
    ui.stirred = ui.clock;
    ui.save_settings();
}

pub fn leave(ui: &mut Ui, id: &str) {
    ui.settings
        .invites
        .retain(|t| Invite::parse(t).map_or(true, |i| i.cache() != id));
    ui.cards.retain(|c| c.id != id);
    ui.save_settings();
    ui.screen = Screen::Settings;
    ui.say("Removed from this computer");
}

pub fn sign_out(ui: &mut Ui, id: &str) {
    let Some(invite) = ui.invite_for(id) else {
        return;
    };
    let invites = ui.settings.invites.clone();
    ui.send(Command::SignOut { invite, invites });
}

pub fn speed_test(ui: &mut Ui, id: &str) {
    let Some(invite) = ui.invite_for(id) else {
        return;
    };
    ui.speed.insert(id.to_owned(), Speed::Testing);
    ui.stirred = ui.clock;
    ui.send(Command::SpeedTest { invite });
}

pub fn show_side(ui: &mut Ui, side: Side) {
    ui.side = side;
}

/// The numbers the detail screen shows for `side`.
pub fn numbers_of(card: &CacheCard, side: Side) -> (&model::Numbers, &[u32]) {
    match side {
        Side::Team => (&card.team, &card.team_recent),
        Side::Mine => (&card.mine, &card.mine_recent),
    }
}
