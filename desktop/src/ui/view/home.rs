use rune::{El, ScrollSpec, Tx, col, el, row, spacer, text};

use super::parts::{self, BODY, CAPTION, HEADING, LABEL, NUMBER, NUMBER_SMALL};
use crate::data::model::{CacheCard, Standing};
use crate::fx;
use crate::ui::actions;
use crate::ui::format;
use crate::ui::palette::Palette;
use crate::ui::state::Ui;

/// The team's numbers over every joined cache: the best round trip, the hit rate over all
/// pulls, and the mean pull.
struct Overall {
    latency: Option<u32>,
    hit_rate: Option<f64>,
    pull_ms: Option<u32>,
    members: usize,
}

fn overall(ui: &Ui) -> Overall {
    let members: Vec<&CacheCard> = ui
        .joined_cards()
        .filter(|c| matches!(c.standing, Standing::Member { .. }))
        .collect();
    let latency = ui.joined_cards().filter_map(|c| c.latency_ms).min();
    let (mut hits, mut asked, mut pull_total, mut pulls) = (0.0, 0.0, 0u64, 0u64);
    for card in &members {
        if let Some(rate) = card.team.hit_rate {
            let weight = f64::from(card.team.pulls.max(1));
            hits += rate * weight;
            asked += weight;
        }
        if let Some(ms) = card.team.pull_ms {
            pull_total += u64::from(ms) * u64::from(card.team.pulls.max(1));
            pulls += u64::from(card.team.pulls.max(1));
        }
    }
    Overall {
        latency,
        hit_rate: (asked > 0.0).then(|| hits / asked),
        pull_ms: (pulls > 0).then(|| u32::try_from(pull_total / pulls).unwrap_or(u32::MAX)),
        members: members.len(),
    }
}

fn hero(ui: &Ui, p: &Palette) -> El {
    let effects = ui.settings.effects;
    let summary = overall(ui);
    let (word, quality) = summary.latency.map_or(("Checking", 1), format::connection);
    let mut card = parts::panel(p)
        .key("hero")
        .padding(18.0)
        .gap(14.0)
        .clip_rounded(16.0);
    if effects {
        card = card.child(
            rune::effect(fx::MESH)
                .effect_data([if p.dark { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0])
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .radius(16.0)
                .opacity(if p.dark { 0.55 } else { 0.7 }),
        );
    }
    let connection = row()
        .gap(14.0)
        .center_y()
        .child(parts::status_dot(p, effects, quality))
        .child(
            text(format!("{word} connection"))
                .key("hero-connection")
                .font(HEADING)
                .color(p.text),
        )
        .child(spacer())
        .child(
            text(summary.latency.map_or("–".into(), |ms| format!("{ms} ms")))
                .key("hero-latency")
                .font(NUMBER_SMALL)
                .color(p.text),
        );
    let figure = |key: &str, name: &str, value: String| {
        col()
            .key(key)
            .grow(1.0)
            .gap(2.0)
            .child(text(name.to_uppercase()).font(LABEL).color(p.muted))
            .child(
                text(value)
                    .key(format!("{key}-value"))
                    .font(NUMBER)
                    .color(p.text),
            )
    };
    let figures = row()
        .gap(12.0)
        .child(figure(
            "hero-hits",
            "Team hit rate",
            summary.hit_rate.map_or("–".into(), format::percent),
        ))
        .child(figure(
            "hero-pull",
            "Average pull",
            summary.pull_ms.map_or("–".into(), format::millis),
        ))
        .child(figure("hero-caches", "Caches", summary.members.to_string()));
    card.child(connection).child(figures)
}

fn size_line(p: &Palette, card: &CacheCard) -> El {
    match (card.live_bytes, card.hard_bytes) {
        (Some(live), Some(hard)) if hard > 0 => {
            let fraction = live as f32 / hard as f32;
            let color = if fraction > 0.9 {
                p.poor
            } else if fraction > 0.75 {
                p.fair
            } else {
                p.accent
            };
            col()
                .gap(6.0)
                .w_full()
                .child(parts::meter(
                    p,
                    &format!("{}-meter", card.id),
                    fraction,
                    color,
                ))
                .child(
                    text(format!(
                        "{} of {}",
                        format::bytes(live),
                        format::bytes(hard)
                    ))
                    .key(format!("{}-size", card.id))
                    .font(CAPTION)
                    .color(p.muted),
                )
        }
        (Some(live), _) => text(format::bytes(live))
            .key(format!("{}-size", card.id))
            .font(CAPTION)
            .color(p.muted),
        _ => el().h(0.0),
    }
}

fn standing_note(card: &CacheCard) -> Option<&str> {
    match &card.standing {
        Standing::Member { .. } | Standing::Loading => None,
        Standing::AwaitingOwner => Some("Waiting for an owner to add this computer"),
        Standing::SignedOut => Some("Sign in to see this cache"),
        Standing::Locked => Some("Unlock to see this cache"),
        Standing::Available => Some("Not joined on this computer"),
        Standing::Trouble(why) => Some(why.as_str()),
    }
}

fn cache_row(ui: &Ui, p: &Palette, card: &CacheCard) -> El {
    let id = card.id.clone();
    let quality = card.latency_ms.map_or(1, |ms| format::connection(ms).1);
    let right = match &card.standing {
        Standing::Member { .. } => row()
            .gap(10.0)
            .center_y()
            .child(parts::status_dot(p, false, quality))
            .child(
                text(card.latency_ms.map_or("–".into(), |ms| format!("{ms} ms")))
                    .font(NUMBER_SMALL.lh(18.0))
                    .color(p.text),
            )
            .child(
                text(
                    card.team
                        .hit_rate
                        .map_or("–".into(), |r| format!("{} hits", format::percent(r))),
                )
                .font(CAPTION)
                .color(p.muted),
            ),
        Standing::Loading => text("Checking…").font(CAPTION).color(p.faint),
        Standing::Available => text("Available").font(LABEL).color(p.accent),
        Standing::Trouble(_) => parts::status_dot(p, false, 0),
        _ => text("Action needed").font(LABEL).color(p.fair),
    };
    let mut body = col()
        .gap(8.0)
        .grow(1.0)
        .min_w(0.0)
        .child(
            row()
                .gap(10.0)
                .center_y()
                .child(
                    text(&card.name)
                        .key(format!("{id}-name"))
                        .font(HEADING)
                        .color(p.text)
                        .truncate()
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(right),
        )
        .child(size_line(p, card));
    if let Some(note) = standing_note(card) {
        body = body.child(
            text(note)
                .key(format!("{id}-note"))
                .font(CAPTION)
                .color(p.muted)
                .wrap(),
        );
    }
    let mut tile = row()
        .key(format!("card-{id}"))
        .matched(format!("card-{id}"))
        .role("button")
        .label(card.name.clone())
        .focusable(true)
        .w_full()
        .gap(12.0)
        .padding(14.0)
        .radius(14.0)
        .fill(p.surface)
        .border(1.0, p.border)
        .hover(p.hover)
        .press_scale(0.985)
        .center_y()
        .child(body)
        .child(rune::icon(&crate::ui::icons::NEXT, 14.0).color(p.faint));
    if ui.settings.effects && card.standing == Standing::Loading {
        tile = tile.child(
            rune::effect(fx::SHIMMER)
                .effect_data([if p.dark { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0])
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .radius(14.0),
        );
    }
    let action = id.clone();
    tile.on_press(
        move |ui: &mut Ui| match ui.card(&action).map(|c| c.standing.clone()) {
            Some(Standing::Available) => actions::join_available(ui, &action),
            Some(Standing::SignedOut) => actions::sign_in_again(ui, &action),
            Some(Standing::AwaitingOwner) => actions::finish_joining(ui, &action),
            Some(Standing::Locked) => ui.screen = crate::ui::state::Screen::Unlock,
            _ => actions::open_detail(ui, &action),
        },
    )
}

fn welcome(ui: &Ui, p: &Palette) -> El {
    col()
        .key("welcome")
        .grow(1.0)
        .center()
        .gap(16.0)
        .padding(24.0)
        .child(parts::orb(p, ui.settings.effects, 84.0))
        .child(
            text("Your team's build cache")
                .font(parts::TITLE)
                .color(p.text),
        )
        .child(
            text("Join with the invite an owner sent you. It takes a minute and three clicks.")
                .font(BODY)
                .color(p.muted)
                .wrap()
                .max_w(300.0),
        )
        .child(parts::primary_button(
            p,
            ui.settings.effects,
            "join",
            "Join a cache",
            true,
            |ui: &mut Ui, _: &mut Tx| actions::start_join(ui),
        ))
}

pub fn view(ui: &Ui, p: &Palette, now: u64) -> El {
    if ui.cards.is_empty() {
        return welcome(ui, p);
    }
    let mut list = col().key("caches").w_full().gap(8.0);
    let mut joined: Vec<&CacheCard> = ui.joined_cards().collect();
    joined.sort_by_key(|c| {
        (
            !matches!(c.standing, Standing::Member { .. }),
            c.name.to_lowercase(),
        )
    });
    for card in joined {
        list = list.child(cache_row(ui, p, card));
    }
    let available: Vec<&CacheCard> = ui.cards.iter().filter(|c| !c.is_joined()).collect();
    if !available.is_empty() {
        list = list.child(
            text("ALSO IN YOUR ORGANISATION")
                .font(LABEL)
                .color(p.faint)
                .mt(10.0),
        );
        for card in available {
            list = list.child(cache_row(ui, p, card));
        }
    }
    let footer = text(match (ui.refreshing, ui.refreshed_at) {
        (true, _) => "Updating…".to_owned(),
        (false, Some(at)) => format!("Updated {}", format::ago(at, now)),
        (false, None) => "Not updated yet".to_owned(),
    })
    .key("updated")
    .font(CAPTION)
    .color(p.faint);
    col()
        .key("home")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .child(
            col()
                .key("home-scroll")
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    col()
                        .w_full()
                        .gap(14.0)
                        .px(16.0)
                        .pb(12.0)
                        .child(hero(ui, p))
                        .child(text("CACHES").font(LABEL).color(p.faint))
                        .child(list),
                ),
        )
        .child(row().px(16.0).py(10.0).child(footer))
}
