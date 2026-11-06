//! The pieces every screen is built from.

use rune::tok::TextStyle;
use rune::{Color, El, Icon, Tx, col, effect, el, icon, row, text};

use crate::fx;
use crate::ui::palette::Palette;
use crate::ui::state::Ui;

pub const NUMBER: TextStyle = TextStyle::new(24.0, 650.0).features("\"tnum\" 1").lh(28.0);
pub const NUMBER_SMALL: TextStyle = TextStyle::new(15.0, 600.0).features("\"tnum\" 1").lh(20.0);
pub const TITLE: TextStyle = TextStyle::new(17.0, 650.0).lh(22.0);
pub const HEADING: TextStyle = TextStyle::new(14.0, 600.0).lh(19.0);
pub const BODY: TextStyle = TextStyle::new(13.0, 400.0).lh(18.0);
pub const LABEL: TextStyle = TextStyle::new(11.5, 600.0).tracking(0.4).lh(14.0);
pub const CAPTION: TextStyle = TextStyle::new(12.0, 450.0).lh(16.0);
pub const CODE: TextStyle = TextStyle::new(12.0, 500.0).mono().lh(17.0);

/// A small round button with an icon; the label is what a screen reader says.
pub fn icon_button(
    p: &Palette,
    key: &str,
    glyph: &'static Icon,
    label: &str,
    on_press: impl Fn(&mut Ui, &mut Tx) + 'static,
) -> El {
    el().key(key)
        .role("button")
        .label(label)
        .help(label)
        .focusable(true)
        .size(30.0, 30.0)
        .radius(15.0)
        .center()
        .hover(p.hover)
        .press_scale(0.92)
        .child(icon(glyph, 16.0).color(p.muted))
        .on_press_tx(on_press)
}

/// The call to action of a screen: accent fill, a liquid chrome rim when effects are on.
pub fn primary_button(
    p: &Palette,
    effects: bool,
    key: &str,
    label: &str,
    enabled: bool,
    on_press: impl Fn(&mut Ui, &mut Tx) + 'static,
) -> El {
    let mut button = row()
        .key(key)
        .role("button")
        .label(label)
        .focusable(enabled)
        .h(38.0)
        .px(18.0)
        .radius(19.0)
        .center()
        .fill(if enabled { p.accent } else { p.track })
        .shadow(6.0, 18.0, p.accent.alpha(if enabled { 0.28 } else { 0.0 }))
        .press_scale(0.97)
        .child(
            text(label)
                .font(HEADING)
                .color(if enabled { p.on_accent } else { p.faint }),
        );
    if effects && enabled {
        button = button.child(
            effect(fx::RIM)
                .effect_data([1.6, 0.0, 0.0, 0.0])
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .radius(19.0),
        );
    }
    if enabled {
        button = button.default_button().on_press_tx(on_press);
    } else {
        button = button.disabled(true);
    }
    button
}

/// A quiet button: text and an optional icon on a faint fill.
pub fn quiet_button(
    p: &Palette,
    key: &str,
    glyph: Option<&'static Icon>,
    label: &str,
    on_press: impl Fn(&mut Ui, &mut Tx) + 'static,
) -> El {
    let mut button = row()
        .key(key)
        .role("button")
        .label(label)
        .focusable(true)
        .h(32.0)
        .px(12.0)
        .gap(6.0)
        .radius(10.0)
        .center()
        .fill(p.track)
        .hover(p.hover)
        .press_scale(0.97);
    if let Some(glyph) = glyph {
        button = button.child(icon(glyph, 14.0).color(p.text));
    }
    button
        .child(text(label).font(LABEL).color(p.text))
        .on_press_tx(on_press)
}

/// The brand mark: a liquid chrome lens, a plain gradient without effects.
pub fn orb(p: &Palette, effects: bool, size: f32) -> El {
    let ring = size * 0.62;
    let mut orb = el()
        .size(size, size)
        .radius(size / 2.0)
        .center()
        .linear_gradient(
            2.4,
            [
                (0.0, Color::WHITE.alpha(0.95)),
                (0.55, p.faint),
                (1.0, p.text.alpha(0.85)),
            ],
        )
        .shadow(size * 0.08, size * 0.3, p.shadow);
    if effects {
        orb = orb.child(
            effect(fx::CHROME)
                .effect_data([if p.dark { 0.0 } else { 1.0 }, 0.0, 0.0, 0.0])
                .absolute(0.0, 0.0)
                .size(size, size)
                .radius(size / 2.0),
        );
    }
    orb.child(
        el().size(ring, ring)
            .radius(ring / 2.0)
            .border((size * 0.045).max(1.0), Color::WHITE.alpha(0.55))
            .shadow(0.0, size * 0.12, p.accent.alpha(0.35)),
    )
}

/// A coloured dot with a radar pulse behind it when effects are on.
pub fn status_dot(p: &Palette, effects: bool, quality: u8) -> El {
    let color = match quality {
        0 => p.poor,
        1 => p.fair,
        _ => p.good,
    };
    let mut dot = el().size(10.0, 10.0).center();
    if effects {
        dot = dot.child(
            effect(fx::PULSE)
                .effect_data([f32::from(quality), 0.0, 0.0, 0.0])
                .effect_bleed(16.0)
                .absolute(0.0, 0.0)
                .size(10.0, 10.0)
                .radius(5.0),
        );
    }
    dot.child(el().size(8.0, 8.0).radius(4.0).fill(color))
}

/// A horizontal meter, `fraction` of it filled.
pub fn meter(p: &Palette, key: &str, fraction: f32, color: Color) -> El {
    let fraction = fraction.clamp(0.0, 1.0);
    el().key(key)
        .h(6.0)
        .w_full()
        .radius(3.0)
        .fill(p.track)
        .child(
            el().h(6.0)
                .w_frac(fraction.max(0.015))
                .radius(3.0)
                .linear_gradient(
                    std::f32::consts::FRAC_PI_2,
                    [(0.0, color.alpha(0.7)), (1.0, color)],
                ),
        )
}

/// Bars of the most recent wall times, the tallest filling the height.
pub fn sparkline(p: &Palette, key: &str, values: &[u32], height: f32) -> El {
    let top = values.iter().copied().max().unwrap_or(0).max(1) as f32;
    let mut bars = row().key(key).h(height).w_full().gap(3.0).items_end();
    for (n, value) in values.iter().enumerate() {
        let h = (*value as f32 / top * height).max(3.0);
        bars = bars.child(
            el().key(format!("{key}-{n}"))
                .grow(1.0)
                .h(h)
                .radius(2.0)
                .fill(
                    p.accent
                        .alpha(0.35 + 0.65 * (n + 1) as f32 / values.len().max(1) as f32),
                ),
        );
    }
    bars
}

/// One number with its name and a quiet note underneath.
pub fn stat(p: &Palette, key: &str, name: &str, value: &str, note: &str) -> El {
    col()
        .key(key)
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .padding(14.0)
        .radius(14.0)
        .fill(p.surface)
        .border(1.0, p.border)
        .child(text(name.to_uppercase()).font(LABEL).color(p.faint))
        .child(
            text(value)
                .key(format!("{key}-value"))
                .font(NUMBER)
                .color(p.text)
                .truncate(),
        )
        .child(text(note).font(CAPTION).color(p.muted).truncate())
}

/// Monospaced text in a well, selectable.
pub fn code_box(p: &Palette, key: &str, content: &str) -> El {
    row()
        .key(key)
        .w_full()
        .gap(10.0)
        .padding(12.0)
        .radius(12.0)
        .fill(p.code)
        .border(1.0, p.hairline)
        .items_start()
        .child(
            text(content)
                .key(format!("{key}-text"))
                .font(CODE)
                .color(p.text)
                .wrap_anywhere()
                .selectable(true)
                .grow(1.0)
                .min_w(0.0),
        )
}

/// The three dots of a three-step flow; the current one is long.
pub fn steps(p: &Palette, current: usize) -> El {
    let mut dots = row().key("steps").gap(6.0).center_y();
    for n in 0..3 {
        let on = n == current;
        dots = dots.child(
            el().key(format!("step-{n}"))
                .h(6.0)
                .w(if on { 20.0 } else { 6.0 })
                .radius(3.0)
                .fill(if n <= current { p.accent } else { p.track }),
        );
    }
    dots
}

/// A rounded panel.
pub fn panel(p: &Palette) -> El {
    col()
        .w_full()
        .radius(16.0)
        .fill(p.surface)
        .border(1.0, p.border)
        .shadow(8.0, 28.0, p.shadow.alpha(0.5))
}

/// A spinner made of the shimmer effect when effects are on, a quiet dot otherwise.
pub fn waiting(p: &Palette, effects: bool, key: &str, label: &str) -> El {
    let mut bar = el().h(4.0).w(140.0).radius(2.0).fill(p.track);
    if effects {
        bar = bar.child(
            effect(fx::SHIMMER)
                .effect_data([if p.dark { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0])
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .radius(2.0),
        );
    } else {
        bar = bar.child(el().h(4.0).w(40.0).radius(2.0).fill(p.accent));
    }
    row()
        .key(key)
        .gap(10.0)
        .center_y()
        .child(bar)
        .child(text(label).font(CAPTION).color(p.muted))
}
