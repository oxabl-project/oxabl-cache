use rune::{El, ScrollSpec, Tx, col, el, row, spacer, text};

use super::parts::{self, CAPTION, HEADING, LABEL};
use crate::data::model::{CacheCard, Standing};
use crate::ui::actions;
use crate::ui::format;
use crate::ui::palette::Palette;
use crate::ui::state::{Side, Speed, Ui};

fn side_switch(p: &Palette, side: Side) -> El {
    let option = |key: &str, label: &str, this: Side| {
        let on = side == this;
        el().key(key)
            .role("radio")
            .label(label)
            .checked(on)
            .focusable(true)
            .h(28.0)
            .grow(1.0)
            .radius(8.0)
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
            .on_press(move |ui: &mut Ui| actions::show_side(ui, this))
    };
    row()
        .key("side")
        .role("radiogroup")
        .focus_group()
        .w_full()
        .padding(3.0)
        .radius(11.0)
        .fill(p.track)
        .child(option("side-team", "Team", Side::Team))
        .child(option("side-mine", "This computer", Side::Mine))
}

fn speed_line(ui: &Ui, p: &Palette, card: &CacheCard) -> El {
    let id = card.id.clone();
    let result = match ui.speed.get(&card.id) {
        Some(Speed::Testing) => parts::waiting(
            p,
            ui.settings.effects,
            "speed-testing",
            "Fetching a sample…",
        ),
        Some(Speed::Measured(Some(rate))) => text(format!("Downloaded at {}", format::rate(*rate)))
            .key("speed-result")
            .font(HEADING)
            .color(p.text),
        Some(Speed::Measured(None)) => text("The cache had nothing to sample yet")
            .key("speed-result")
            .font(CAPTION)
            .color(p.muted),
        None => text("Fetches a few entries and verifies them, as a build does")
            .font(CAPTION)
            .color(p.muted)
            .wrap(),
    };
    row()
        .key("speed")
        .w_full()
        .gap(12.0)
        .center_y()
        .child(result.grow(1.0).min_w(0.0))
        .child(parts::quiet_button(
            p,
            "speed-test",
            Some(&crate::ui::icons::BOLT),
            "Test speed",
            move |ui: &mut Ui, _: &mut Tx| actions::speed_test(ui, &id),
        ))
}

pub fn view(ui: &Ui, p: &Palette, id: &str) -> El {
    let Some(card) = ui.card(id) else {
        return col().grow(1.0).center().child(
            text("This cache is no longer here")
                .font(HEADING)
                .color(p.muted),
        );
    };
    let (numbers, recent) = actions::numbers_of(card, ui.side);
    let role = match &card.standing {
        Standing::Member { role } => role.clone(),
        _ => "not a member yet".into(),
    };
    let none = |text: &str| text.to_owned();
    let latency = match ui.side {
        Side::Team => numbers.rtt_ms.or(card.latency_ms),
        Side::Mine => card.latency_ms.or(numbers.rtt_ms),
    };
    let tiles_top = row()
        .gap(10.0)
        .child(parts::stat(
            p,
            "stat-latency",
            "Latency",
            &latency.map_or(none("–"), |ms| format!("{ms} ms")),
            &latency.map_or(none("not measured"), |ms| {
                format!("{} connection", format::connection(ms).0.to_lowercase())
            }),
        ))
        .child(parts::stat(
            p,
            "stat-hits",
            "Hit rate",
            &numbers.hit_rate.map_or(none("–"), format::percent),
            &format!("over {} pulls", numbers.pulls),
        ));
    let tiles_bottom = row()
        .gap(10.0)
        .child(parts::stat(
            p,
            "stat-pull",
            "Average pull",
            &numbers.pull_ms.map_or(none("–"), format::millis),
            &numbers
                .pull_bytes_per_second
                .map_or(none("no pulls yet"), format::rate),
        ))
        .child(parts::stat(
            p,
            "stat-push",
            "Average push",
            &numbers.push_ms.map_or(none("–"), format::millis),
            &format!("over {} pushes", numbers.pushes),
        ));
    let chart = if recent.is_empty() {
        text(match ui.side {
            Side::Team => "No builds have shared numbers yet",
            Side::Mine => "No builds on this computer yet",
        })
        .key("chart-empty")
        .font(CAPTION)
        .color(p.faint)
    } else {
        col()
            .gap(8.0)
            .w_full()
            .child(
                row()
                    .child(text("RECENT PULLS").font(LABEL).color(p.faint))
                    .child(spacer())
                    .child(
                        text(format!(
                            "{} max",
                            format::millis(recent.iter().copied().max().unwrap_or(0))
                        ))
                        .font(CAPTION)
                        .color(p.faint),
                    ),
            )
            .child(parts::sparkline(p, "chart", recent, 44.0))
    };
    let size = match (card.live_bytes, card.hard_bytes) {
        (Some(live), Some(hard)) if hard > 0 => col()
            .gap(6.0)
            .w_full()
            .child(
                row()
                    .child(text("STORAGE").font(LABEL).color(p.faint))
                    .child(spacer())
                    .child(
                        text(format!(
                            "{} of {}",
                            format::bytes(live),
                            format::bytes(hard)
                        ))
                        .key("detail-size")
                        .font(CAPTION)
                        .color(p.muted),
                    ),
            )
            .child(parts::meter(
                p,
                "detail-meter",
                live as f32 / hard as f32,
                p.accent,
            )),
        _ => el().h(0.0),
    };
    let header = col()
        .gap(4.0)
        .child(
            text(&card.name)
                .key("detail-name")
                .font(parts::TITLE)
                .color(p.text)
                .truncate(),
        )
        .child(
            text(format!("{} · {role}", card.host))
                .key("detail-host")
                .font(CAPTION)
                .color(p.muted),
        );
    let body = col()
        .key("detail")
        .matched(format!("card-{id}"))
        .w_full()
        .gap(14.0)
        .padding(16.0)
        .radius(16.0)
        .fill(p.surface)
        .border(1.0, p.border)
        .child(header)
        .child(side_switch(p, ui.side))
        .child(tiles_top)
        .child(tiles_bottom)
        .child(chart)
        .child(size)
        .child(el().h(1.0).w_full().fill(p.hairline))
        .child(speed_line(ui, p, card));
    col()
        .key("detail-screen")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(col().w_full().px(16.0).pb(16.0).child(body))
}
