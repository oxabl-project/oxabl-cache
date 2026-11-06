use rune::{El, Tx, col, el, row, text, text_input};

use super::parts::{self, BODY, CAPTION, TITLE};
use crate::ui::actions;
use crate::ui::palette::Palette;
use crate::ui::state::Ui;

pub fn view(ui: &Ui, p: &Palette) -> El {
    let field = row()
        .w_full()
        .h(40.0)
        .px(12.0)
        .center_y()
        .radius(10.0)
        .fill(p.code)
        .border(
            1.0,
            if ui.unlock.error.is_some() {
                p.poor.alpha(0.6)
            } else {
                p.border
            },
        )
        .child(
            text_input(&ui.unlock.edit, parts::CODE)
                .key("passphrase-field")
                .label("Passphrase")
                .single_line()
                .secure()
                .grow(1.0)
                .min_w(0.0)
                .color(p.text)
                .placeholder("Passphrase", p.faint)
                .on_edit(|ui: &mut Ui| &mut ui.unlock.edit)
                .on_submit(|ui: &mut Ui, _: &mut Tx| actions::unlock(ui)),
        );
    let remember = row()
        .key("remember")
        .role("checkbox")
        .label("Remember on this computer")
        .checked(ui.unlock.remember)
        .focusable(true)
        .gap(8.0)
        .center_y()
        .child(
            el().size(16.0, 16.0)
                .radius(5.0)
                .center()
                .fill(if ui.unlock.remember {
                    p.accent
                } else {
                    p.track
                })
                .child(if ui.unlock.remember {
                    rune::icon(&crate::ui::icons::CHECK, 12.0).color(p.on_accent)
                } else {
                    el()
                }),
        )
        .child(
            text("Remember on this computer")
                .font(CAPTION)
                .color(p.text),
        )
        .on_press(|ui: &mut Ui| ui.unlock.remember = !ui.unlock.remember);
    let mut panel = parts::panel(p)
        .key("unlock")
        .padding(20.0)
        .gap(14.0)
        .child(parts::orb(p, ui.settings.effects, 44.0))
        .child(text("Unlock this computer").font(TITLE).color(p.text))
        .child(
            text("Your identity is locked and the keyring has no passphrase for it. Paste it from your password manager.")
                .font(BODY)
                .color(p.muted)
                .wrap(),
        )
        .child(field)
        .child(remember);
    if let Some(why) = &ui.unlock.error {
        panel = panel.child(
            text(why)
                .key("unlock-error")
                .font(CAPTION)
                .color(p.poor)
                .wrap(),
        );
    }
    panel = if ui.unlock.busy {
        panel.child(parts::waiting(
            p,
            ui.settings.effects,
            "unlocking",
            "Unlocking…",
        ))
    } else {
        panel.child(row().child(parts::primary_button(
            p,
            ui.settings.effects,
            "unlock-button",
            "Unlock",
            true,
            |ui: &mut Ui, _: &mut Tx| actions::unlock(ui),
        )))
    };
    col()
        .key("unlock-screen")
        .grow(1.0)
        .w_full()
        .px(16.0)
        .pb(16.0)
        .child(panel)
}
