//! The window: a header, the current screen and a toast.

mod detail;
mod home;
mod join;
pub mod parts;
mod settings;
mod unlock;

use rune::{Cx, Edge, El, Transition, Tx, col, el, row, spacer, text};

use super::actions;
use super::icons;
use super::palette::{Palette, palette};
use super::state::{Screen, Ui};
use crate::data::settings::Appearance;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn title_of(ui: &Ui) -> String {
    match &ui.screen {
        Screen::Home => "Oxabl Cache".into(),
        Screen::Detail(_) => "Cache".into(),
        Screen::Join => "Join".into(),
        Screen::Settings => "Settings".into(),
        Screen::Unlock => "Unlock".into(),
    }
}

fn header(ui: &Ui, p: &Palette) -> El {
    let home = ui.screen == Screen::Home;
    let leading = if home {
        parts::orb(p, ui.settings.effects, 26.0)
    } else {
        parts::icon_button(
            p,
            "back",
            &icons::BACK,
            "Back",
            |ui: &mut Ui, tx: &mut Tx| actions::back(ui, tx),
        )
    };
    let subtitle = if home {
        let joined = ui.joined_cards().count();
        let hosts: std::collections::BTreeSet<&str> =
            ui.joined_cards().map(|c| c.host.as_str()).collect();
        match (joined, hosts.len()) {
            (0, _) => "No caches yet".to_owned(),
            (1, _) => format!("1 cache · {}", hosts.iter().next().copied().unwrap_or("")),
            (n, 1) => format!(
                "{n} caches · {}",
                hosts.iter().next().copied().unwrap_or("")
            ),
            (n, h) => format!("{n} caches · {h} organisations"),
        }
    } else {
        String::new()
    };
    let mut titles = col()
        .key("titles")
        .gap(1.0)
        .grow(1.0)
        .min_w(0.0)
        .window_drag()
        .child(
            text(title_of(ui))
                .key("title")
                .font(parts::HEADING)
                .color(p.text),
        );
    if !subtitle.is_empty() {
        titles = titles.child(
            text(subtitle)
                .key("subtitle")
                .font(parts::CAPTION)
                .color(p.muted)
                .truncate(),
        );
    }
    let mut trailing = row().gap(2.0).center_y();
    if home && !ui.cards.is_empty() {
        trailing = trailing
            .child(parts::icon_button(
                p,
                "refresh",
                &icons::REFRESH,
                "Refresh",
                |ui: &mut Ui, _: &mut Tx| actions::refresh(ui),
            ))
            .child(parts::icon_button(
                p,
                "add",
                &icons::PLUS,
                "Join a cache",
                |ui: &mut Ui, _: &mut Tx| actions::start_join(ui),
            ));
    }
    if ui.screen != Screen::Settings && ui.screen != Screen::Unlock {
        trailing = trailing.child(parts::icon_button(
            p,
            "settings",
            &icons::GEAR,
            "Settings",
            |ui: &mut Ui, _: &mut Tx| ui.screen = Screen::Settings,
        ));
    }
    row()
        .key("header")
        .w_full()
        .h(60.0)
        .px(16.0)
        .gap(12.0)
        .center_y()
        .window_drag()
        .child(leading)
        .child(titles)
        .child(spacer().window_drag())
        .child(trailing)
}

fn toast(ui: &Ui, p: &Palette) -> El {
    match &ui.toast {
        Some((message, _)) => row()
            .key("toast")
            .w_full()
            .px(16.0)
            .pb(14.0)
            .center()
            .transition(Transition::offset(0.0, 12.0).combined(Transition::opacity()))
            .child(
                row()
                    .gap(8.0)
                    .px(14.0)
                    .h(34.0)
                    .radius(17.0)
                    .center_y()
                    .fill(p.raised)
                    .border(1.0, p.border)
                    .shadow(6.0, 20.0, p.shadow)
                    .child(rune::icon(&icons::CHECK, 13.0).color(p.accent))
                    .child(
                        text(message)
                            .key("toast-text")
                            .font(parts::CAPTION)
                            .color(p.text),
                    ),
            ),
        None => el().h(0.0),
    }
}

/// Light or dark: the setting, else the desktop's preference.
pub fn is_dark(ui: &Ui, cx: &Cx<Ui>) -> bool {
    match ui.settings.appearance {
        Appearance::System => cx.system_dark(),
        Appearance::Light => false,
        Appearance::Dark => true,
    }
}

pub fn view(ui: &Ui, cx: &mut Cx<Ui>) -> El {
    let p = palette(is_dark(ui, cx));
    let (screen, edge) = match &ui.screen {
        Screen::Home => (home::view(ui, &p, now()), Edge::Leading),
        Screen::Detail(id) => (detail::view(ui, &p, id), Edge::Trailing),
        Screen::Join => (join::view(ui, &p), Edge::Bottom),
        Screen::Settings => (settings::view(ui, &p), Edge::Trailing),
        Screen::Unlock => (unlock::view(ui, &p), Edge::Bottom),
    };
    let key = match &ui.screen {
        Screen::Detail(id) => format!("screen-detail-{id}"),
        other => format!("screen-{other:?}").to_lowercase(),
    };
    col()
        .key("window")
        .w_full()
        .h_full()
        .fill(p.backdrop)
        .radius(18.0)
        .clip_rounded(18.0)
        .border(1.0, p.border)
        .child(header(ui, &p))
        .child(
            col()
                .key(key)
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .transition(Transition::push(edge))
                .child(screen),
        )
        .child(toast(ui, &p))
}
