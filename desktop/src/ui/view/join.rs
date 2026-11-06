use oxabl_cache_client::Invite;
use rune::{El, Tx, col, effect, el, row, text, text_input};

use super::parts::{self, BODY, CAPTION, HEADING, TITLE};
use crate::fx;
use crate::ui::actions;
use crate::ui::palette::Palette;
use crate::ui::state::{JoinStep, Ui};

fn heading(p: &Palette, title: &str, caption: &str) -> El {
    col()
        .gap(6.0)
        .child(text(title).key("join-title").font(TITLE).color(p.text))
        .child(text(caption).font(BODY).color(p.muted).wrap())
}

fn problem(p: &Palette, why: &Option<String>) -> El {
    match why {
        Some(why) => text(why)
            .key("join-error")
            .font(CAPTION)
            .color(p.poor)
            .wrap(),
        None => el().h(0.0),
    }
}

fn invite_step(ui: &Ui, p: &Palette) -> El {
    let field = row()
        .w_full()
        .h(40.0)
        .px(12.0)
        .gap(8.0)
        .center_y()
        .radius(10.0)
        .fill(p.code)
        .border(
            1.0,
            if ui.join.error.is_some() {
                p.poor.alpha(0.6)
            } else {
                p.border
            },
        )
        .child(
            text_input(&ui.join.edit, parts::CODE)
                .key("invite-field")
                .label("Invite")
                .single_line()
                .grow(1.0)
                .min_w(0.0)
                .color(p.text)
                .placeholder("oxc-invite:…", p.faint)
                .on_edit(|ui: &mut Ui| &mut ui.join.edit)
                .on_input_change(|ui: &mut Ui, _: &mut Tx| ui.join.error = None)
                .on_submit(|ui: &mut Ui, _: &mut Tx| actions::continue_join(ui)),
        )
        .child(parts::quiet_button(
            p,
            "paste-invite",
            Some(&crate::ui::icons::PASTE),
            "Paste",
            |_: &mut Ui, tx: &mut Tx| actions::paste_invite(tx),
        ));
    let preview = match Invite::parse(&ui.join.edit.text) {
        Ok(invite) => row()
            .key("invite-preview")
            .gap(12.0)
            .padding(12.0)
            .radius(12.0)
            .fill(p.accent_soft)
            .center_y()
            .child(parts::orb(p, false, 28.0))
            .child(
                col()
                    .gap(2.0)
                    .grow(1.0)
                    .min_w(0.0)
                    .child(
                        text(invite.name())
                            .key("invite-name")
                            .font(HEADING)
                            .color(p.text)
                            .truncate(),
                    )
                    .child(
                        text(invite.server().host_str().unwrap_or(""))
                            .font(CAPTION)
                            .color(p.muted)
                            .truncate(),
                    ),
            ),
        Err(_) => el().h(0.0),
    };
    col()
        .gap(14.0)
        .w_full()
        .child(heading(
            p,
            "Join a cache",
            "Paste the invite an owner of the cache sent you.",
        ))
        .child(field)
        .child(preview)
        .child(problem(p, &ui.join.error))
        .child(row().child(parts::primary_button(
            p,
            ui.settings.effects,
            "continue",
            "Continue",
            true,
            |ui: &mut Ui, _: &mut Tx| actions::continue_join(ui),
        )))
}

fn sign_in_step(ui: &Ui, p: &Palette, code: &Option<String>, failed: &Option<String>) -> El {
    let host = ui
        .join
        .invite
        .as_ref()
        .and_then(|i| i.server().host_str().map(str::to_owned))
        .unwrap_or_default();
    let mut step = col().gap(14.0).w_full().child(heading(
        p,
        "Sign in",
        &format!("Approve this computer with your organisation's sign-in for {host}."),
    ));
    if let Some(why) = failed {
        return step
            .child(problem(p, &Some(why.clone())))
            .child(row().child(parts::primary_button(
                p,
                ui.settings.effects,
                "try-again",
                "Try again",
                true,
                |ui: &mut Ui, _: &mut Tx| actions::begin_sign_in(ui),
            )));
    }
    match code {
        None => step.child(parts::waiting(
            p,
            ui.settings.effects,
            "sign-in-starting",
            "Starting the sign-in…",
        )),
        Some(code) => {
            step = step
                .child(
                    row()
                        .key("sign-in-code")
                        .gap(12.0)
                        .padding(14.0)
                        .radius(14.0)
                        .fill(p.code)
                        .border(1.0, p.hairline)
                        .center_y()
                        .child(
                            text(code)
                                .key("sign-in-code-text")
                                .font(parts::NUMBER.mono())
                                .color(p.text)
                                .grow(1.0),
                        )
                        .child(parts::icon_button(
                            p,
                            "copy-code",
                            &crate::ui::icons::COPY,
                            "Copy code",
                            |ui: &mut Ui, tx: &mut Tx| actions::copy_code(ui, tx),
                        )),
                )
                .child(row().gap(10.0).center_y().child(parts::primary_button(
                    p,
                    ui.settings.effects,
                    "open-sign-in",
                    "Open sign-in page",
                    true,
                    |ui: &mut Ui, tx: &mut Tx| actions::open_sign_in_page(ui, tx),
                )))
                .child(parts::waiting(
                    p,
                    ui.settings.effects,
                    "sign-in-waiting",
                    "Waiting for you to approve…",
                ));
            step
        }
    }
}

fn request_step(
    ui: &Ui,
    p: &Palette,
    command: &Option<String>,
    has_passphrase: bool,
    failed: &Option<String>,
) -> El {
    let name = ui
        .join
        .invite
        .as_ref()
        .map(|i| i.name().to_owned())
        .unwrap_or_default();
    let mut step = col()
        .gap(14.0)
        .w_full()
        .child(heading(p, "Ask to be added", &format!("Send this to an owner of {name}. They run it once, and this window carries on by itself.")));
    match command {
        None => {
            step = step.child(parts::waiting(
                p,
                ui.settings.effects,
                "request-preparing",
                "Making this computer's key…",
            ))
        }
        Some(command) => {
            step = step
                .child(parts::code_box(p, "request", command))
                .child(row().child(parts::primary_button(
                    p,
                    ui.settings.effects,
                    "copy-request",
                    "Copy request",
                    true,
                    |ui: &mut Ui, tx: &mut Tx| actions::copy_request(ui, tx),
                )));
        }
    }
    if has_passphrase {
        step = step.child(
            row()
                .key("passphrase-note")
                .gap(12.0)
                .padding(12.0)
                .radius(12.0)
                .fill(p.accent_soft)
                .items_start()
                .child(rune::icon(&crate::ui::icons::LOCK, 16.0).color(p.accent).mt(2.0))
                .child(
                    col().gap(10.0).grow(1.0).min_w(0.0)
                        .child(
                            text("Your new passphrase is in this computer's keyring. Keep a copy in your password manager too.")
                                .font(CAPTION)
                                .color(p.text)
                                .wrap(),
                        )
                        .child(row().child(parts::quiet_button(p, "copy-passphrase", Some(&crate::ui::icons::COPY), "Copy passphrase", |ui: &mut Ui, tx: &mut Tx| {
                            actions::copy_new_passphrase(ui, tx)
                        }))),
                ),
        );
    }
    match failed {
        Some(why) => step.child(problem(p, &Some(why.clone()))),
        None if command.is_some() => step.child(parts::waiting(
            p,
            ui.settings.effects,
            "request-waiting",
            "Waiting for an owner to add you…",
        )),
        None => step,
    }
}

fn done_step(ui: &Ui, p: &Palette) -> El {
    let name = ui
        .join
        .invite
        .as_ref()
        .map(|i| i.name().to_owned())
        .unwrap_or_default();
    let mut mark = el()
        .size(84.0, 84.0)
        .center()
        .child(parts::orb(p, ui.settings.effects, 84.0));
    if ui.settings.effects {
        mark = mark.child(
            effect(fx::SPARKLES)
                .effect_data([ui.stirred as f32, 0.0, 0.0, 0.0])
                .effect_bleed(60.0)
                .absolute(0.0, 0.0)
                .size(84.0, 84.0)
                .radius(42.0),
        );
    }
    col()
        .key("joined")
        .w_full()
        .gap(14.0)
        .items_center()
        .child(mark)
        .child(
            text("You're in")
                .key("join-title")
                .font(TITLE)
                .color(p.text),
        )
        .child(
            text(format!("This computer now reads and writes {name}."))
                .font(BODY)
                .color(p.muted)
                .wrap(),
        )
        .child(parts::primary_button(
            p,
            ui.settings.effects,
            "done",
            "Done",
            true,
            |ui: &mut Ui, _: &mut Tx| {
                actions::home(ui);
                actions::refresh(ui);
            },
        ))
}

pub fn view(ui: &Ui, p: &Palette) -> El {
    let (index, content) = match &ui.join.step {
        JoinStep::Invite => (0, invite_step(ui, p)),
        JoinStep::SignIn { code, failed, .. } => (1, sign_in_step(ui, p, code, failed)),
        JoinStep::Request {
            command,
            passphrase,
            failed,
        } => (
            2,
            request_step(ui, p, command, passphrase.is_some(), failed),
        ),
        JoinStep::Done => (2, done_step(ui, p)),
    };
    col()
        .key("join-screen")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .px(16.0)
        .pb(16.0)
        .child(
            parts::panel(p)
                .key(format!(
                    "join-step-{index}-{}",
                    matches!(ui.join.step, JoinStep::Done)
                ))
                .padding(20.0)
                .gap(18.0)
                .transition(rune::Transition::push(rune::Edge::Trailing))
                .child(parts::steps(p, index))
                .child(content),
        )
}
