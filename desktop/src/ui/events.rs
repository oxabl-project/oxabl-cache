use crate::data::model::Standing;
use crate::data::settings::save_snapshot;
use crate::data::worker::Event;

use super::state::{JoinStep, Screen, Speed, Ui};

fn place_card(ui: &mut Ui, card: crate::data::model::CacheCard) {
    match ui.cards.iter_mut().find(|c| c.id == card.id) {
        Some(slot) => {
            let keep_size = card.live_bytes.is_none() && slot.live_bytes.is_some();
            let (live, hard) = (slot.live_bytes, slot.hard_bytes);
            *slot = card;
            if keep_size {
                slot.live_bytes = live;
                slot.hard_bytes = hard;
            }
        }
        None => ui.cards.push(card),
    }
}

/// Applies what the background sent.
pub fn apply(ui: &mut Ui, event: Event, tx: &mut rune::Tx) {
    match event {
        Event::Card(card) => place_card(ui, *card),
        Event::Refreshed { at, ids } => {
            ui.cards.retain(|c| ids.contains(&c.id));
            ui.refreshing = false;
            ui.refreshed_at = Some(at);
            let _ = save_snapshot(&ui.paths.snapshot_file(), &ui.cards);
        }
        Event::NeedsUnlock => {
            ui.needs_unlock = true;
            if ui.screen == Screen::Home && !ui.settings.invites.is_empty() {
                ui.screen = Screen::Unlock;
            }
        }
        Event::Unlocked => {
            ui.needs_unlock = false;
            ui.unlock.busy = false;
            ui.unlock.error = None;
            ui.unlock.edit = rune::TextEdit::new("");
            if ui.screen == Screen::Unlock {
                ui.screen = Screen::Home;
            }
        }
        Event::UnlockFailed(why) => {
            ui.unlock.busy = false;
            ui.unlock.error = Some(why);
            ui.unlock.edit = rune::TextEdit::new("");
            ui.unlock.edit.focused = true;
        }
        Event::SignInCode { url, code } => {
            if let JoinStep::SignIn {
                url: u, code: c, ..
            } = &mut ui.join.step
            {
                *u = Some(url);
                *c = Some(code);
            }
        }
        Event::SignedIn => {
            if matches!(
                ui.join.step,
                JoinStep::Checking | JoinStep::SignIn { .. } | JoinStep::Token { .. }
            ) {
                super::actions::begin_request(ui);
            } else {
                ui.say("Signed in");
                super::actions::refresh(ui);
            }
        }
        Event::NeedsSignIn => {
            if matches!(ui.join.step, JoinStep::Checking) {
                super::actions::begin_sign_in(ui);
            }
        }
        Event::NeedsToken(why) => {
            if ui.screen == Screen::Join
                && !matches!(ui.join.step, JoinStep::Invite | JoinStep::Done)
            {
                super::actions::ask_for_token(ui, why);
            } else if let Some(why) = why {
                ui.say(why);
            }
        }
        Event::TokenRefused(why) => {
            if let JoinStep::Token { busy, failed } = &mut ui.join.step {
                *busy = false;
                *failed = Some(why);
                ui.join.token = rune::TextEdit::new("");
                ui.join.token.focused = true;
            }
        }
        Event::SignInFailed(why) => {
            if let JoinStep::SignIn { failed, .. } = &mut ui.join.step {
                *failed = Some(why);
            } else {
                ui.say(why);
            }
        }
        Event::JoinRequest {
            command,
            new_passphrase,
        } => {
            if let JoinStep::Request {
                command: c,
                passphrase,
                ..
            } = &mut ui.join.step
            {
                *c = Some(command);
                *passphrase = new_passphrase;
            }
        }
        Event::Joined { cache } => {
            if let Some(card) = ui.cards.iter_mut().find(|c| c.id == cache) {
                card.standing = Standing::Loading;
            }
            if matches!(ui.join.step, JoinStep::Request { .. }) {
                ui.join.step = JoinStep::Done;
                ui.stirred = ui.clock;
            }
            super::actions::refresh(ui);
        }
        Event::JoinFailed(why) => {
            if let JoinStep::Request { failed, .. } = &mut ui.join.step {
                *failed = Some(why);
            } else {
                ui.say(why);
            }
        }
        Event::Passphrase(passphrase) => super::actions::copy_secret(ui, tx, &passphrase),
        Event::Notice(text) => ui.say(text),
        Event::Speed {
            cache,
            bytes_per_second,
        } => {
            ui.speed.insert(cache, Speed::Measured(bytes_per_second));
        }
    }
}
