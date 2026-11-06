//! The window: state, what presses do, how background news lands, and the view.

pub mod actions;
pub mod demo;
pub mod events;
pub mod format;
pub mod icons;
pub mod palette;
pub mod state;
pub mod view;

use rune::{Key, KeyEvent, Tx};

use crate::data::worker::Command;
use state::{JoinStep, Speed, Ui};

/// Refreshes this often while the window is open, in seconds.
const REFRESH_EVERY: f64 = 120.0;

impl Ui {
    /// Whether something is under way that the person is watching.
    pub fn is_waiting(&self) -> bool {
        self.speed.values().any(|s| *s == Speed::Testing)
            || matches!(
                self.join.step,
                JoinStep::SignIn { failed: None, .. } | JoinStep::Request { failed: None, .. }
            ) && self.screen == state::Screen::Join
    }
}

/// Runs every frame: lands background news, expires the toast, refreshes now and then.
pub fn poll(ui: &mut Ui, t: f64, tx: &mut Tx) -> bool {
    ui.clock = t;
    let mut changed = false;
    let events = ui.worker.as_ref().map(|w| w.drain()).unwrap_or_default();
    for event in events {
        events::apply(ui, event, tx);
        changed = true;
    }
    if ui.toast.as_ref().is_some_and(|(_, until)| *until <= t) {
        ui.toast = None;
        changed = true;
    }
    if ui.worker.is_some() && !ui.refreshing && t - ui.last_refresh > REFRESH_EVERY {
        actions::refresh(ui);
        changed = true;
    }
    if let Some((_, until)) = &ui.toast {
        tx.wake_after(std::time::Duration::from_secs_f64((until - t).max(0.05)));
    } else if ui.worker.is_some() {
        tx.wake_after(std::time::Duration::from_secs_f64(
            (ui.last_refresh + REFRESH_EVERY - t).max(1.0),
        ));
    }
    changed
}

/// Escape goes back, and closes the window from home; Ctrl or Cmd with R refreshes, N joins and
/// the comma opens the settings.
pub fn on_key(ui: &mut Ui, event: &KeyEvent, tx: &mut Tx) -> bool {
    match event.key {
        Key::Escape => {
            actions::back(ui, tx);
            true
        }
        Key::Command('r') => {
            actions::refresh(ui);
            true
        }
        Key::Command('n') => {
            actions::start_join(ui);
            true
        }
        Key::Command(',') => {
            ui.screen = state::Screen::Settings;
            true
        }
        _ => false,
    }
}

pub fn send(ui: &Ui, command: Command) {
    ui.send(command);
}
