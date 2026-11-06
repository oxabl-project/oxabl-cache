use rune::{El, ScrollSpec, Tx, col, el, row, spacer, text};

use super::parts::{self, BODY, CAPTION, HEADING, LABEL};
use crate::data::machine_label;
use crate::data::settings::Appearance;
use crate::ui::actions;
use crate::ui::palette::Palette;
use crate::ui::state::Ui;

fn section(p: &Palette, title: &str) -> El {
    text(title.to_uppercase())
        .font(LABEL)
        .color(p.faint)
        .mt(6.0)
}

fn line(p: &Palette, key: &str, label: &str, control: El) -> El {
    row()
        .key(key)
        .w_full()
        .gap(12.0)
        .min_h(44.0)
        .px(14.0)
        .center_y()
        .child(text(label).font(BODY).color(p.text).grow(1.0).min_w(0.0))
        .child(control)
}

fn appearance(p: &Palette, current: Appearance) -> El {
    let option = |key: &str, label: &str, this: Appearance| {
        let on = current == this;
        el().key(key)
            .role("radio")
            .label(label)
            .checked(on)
            .focusable(true)
            .h(26.0)
            .px(12.0)
            .radius(7.0)
            .center()
            .fill(if on {
                p.raised
            } else {
                rune::Color::TRANSPARENT
            })
            .shadow(1.0, 4.0, p.shadow.alpha(if on { 0.4 } else { 0.0 }))
            .child(
                text(label)
                    .font(LABEL)
                    .color(if on { p.text } else { p.muted }),
            )
            .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| actions::set_appearance(ui, tx, this))
    };
    row()
        .key("appearance")
        .role("radiogroup")
        .focus_group()
        .padding(3.0)
        .radius(10.0)
        .fill(p.track)
        .child(option("appearance-system", "System", Appearance::System))
        .child(option("appearance-light", "Light", Appearance::Light))
        .child(option("appearance-dark", "Dark", Appearance::Dark))
}

fn switch(p: &Palette, key: &str, label: &str, on: bool, toggle: impl Fn(&mut Ui) + 'static) -> El {
    el().key(key)
        .role("switch")
        .label(label)
        .checked(on)
        .focusable(true)
        .size(38.0, 22.0)
        .radius(11.0)
        .fill(if on { p.accent } else { p.track })
        .child(
            el().size(18.0, 18.0)
                .radius(9.0)
                .fill(rune::Color::WHITE)
                .shadow(1.0, 3.0, p.shadow)
                .absolute(if on { 18.0 } else { 2.0 }, 2.0),
        )
        .motion(rune::tok::motion::SNAPPY)
        .on_press(toggle)
}

pub fn view(ui: &Ui, p: &Palette) -> El {
    let group = |children: Vec<El>| {
        let mut panel = parts::panel(p).py(4.0);
        let count = children.len();
        for (n, child) in children.into_iter().enumerate() {
            panel = panel.child(child);
            if n + 1 < count {
                panel = panel.child(el().h(1.0).mx(14.0).fill(p.hairline));
            }
        }
        panel
    };
    let look = group(vec![
        line(
            p,
            "line-appearance",
            "Appearance",
            appearance(p, ui.settings.appearance),
        ),
        line(
            p,
            "line-effects",
            "Liquid metal and motion",
            switch(
                p,
                "effects",
                "Effects",
                ui.settings.effects,
                actions::toggle_effects,
            ),
        ),
    ]);
    let identity = group(vec![
        line(
            p,
            "line-label",
            "This computer",
            text(machine_label()).font(CAPTION).color(p.muted),
        ),
        line(
            p,
            "line-passphrase",
            "Identity passphrase",
            parts::quiet_button(
                p,
                "settings-copy-passphrase",
                Some(&crate::ui::icons::COPY),
                "Copy",
                |ui: &mut Ui, _: &mut Tx| actions::copy_stored_passphrase(ui),
            ),
        ),
    ]);
    let mut caches = Vec::new();
    for card in ui.joined_cards() {
        let (leave, out) = (card.id.clone(), card.id.clone());
        caches.push(
            row()
                .key(format!("manage-{}", card.id))
                .w_full()
                .gap(8.0)
                .min_h(48.0)
                .px(14.0)
                .center_y()
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(text(&card.name).font(HEADING).color(p.text).truncate())
                        .child(text(&card.host).font(CAPTION).color(p.muted).truncate()),
                )
                .child(parts::quiet_button(
                    p,
                    &format!("sign-out-{}", card.id),
                    None,
                    "Sign out",
                    move |ui: &mut Ui, _: &mut Tx| actions::sign_out(ui, &out),
                ))
                .child(parts::quiet_button(
                    p,
                    &format!("leave-{}", card.id),
                    None,
                    "Remove",
                    move |ui: &mut Ui, _: &mut Tx| actions::leave(ui, &leave),
                )),
        );
    }
    let mut content = col()
        .w_full()
        .gap(8.0)
        .px(16.0)
        .pb(16.0)
        .child(section(p, "Look"))
        .child(look)
        .child(section(p, "Identity"))
        .child(identity);
    if !caches.is_empty() {
        content = content
            .child(section(p, "Caches on this computer"))
            .child(group(caches));
    }
    content = content.child(
        row().mt(8.0).child(spacer()).child(
            text(concat!("Oxabl Cache ", env!("CARGO_PKG_VERSION")))
                .font(CAPTION)
                .color(p.faint),
        ),
    );
    col()
        .key("settings-screen")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content)
}
