//! The compositor: every `El::effect` slot the view places becomes one rune-fx layer. The slot's
//! mark names the effect and its `effect_data` carries the few numbers that vary.

use rune::compose::{Compositor, Frame};
use rune::render::{EffectPhase, EffectSite, KIND_EFFECT};
use rune_fx::{
    FillAnchor, Fx, FxFrame, FxLayer, FxOptions, FxRenderer, FxSlot, LiquidMetalParams,
    MeshGradientParams, PulseParams, PulseStyle, Shape, ShimmerParams, SparkleParams, Stops, rgb,
};

use crate::ui::state::Ui;

/// A liquid chrome face. Data: `[0]` 1 for a lighter studio in light appearance.
pub const CHROME: u32 = 1;
/// A liquid chrome rim. Data: `[0]` rim width in points.
pub const RIM: u32 = 2;
/// A slow mesh gradient. Data: `[0]` 1 in dark appearance.
pub const MESH: u32 = 3;
/// A radar pulse. Data: `[0]` quality 0 poor, 1 fair, 2 good.
pub const PULSE: u32 = 4;
/// A loading shimmer. Data: `[0]` 1 in dark appearance.
pub const SHIMMER: u32 = 5;
/// A burst of sparkles. Data: `[0]` the burst's start on the window clock.
pub const SPARKLES: u32 = 6;

/// Seconds the effects keep moving after something changed.
const AWAKE_SECONDS: f64 = 8.0;
/// The first frames draw without effects: compiling their pipelines would hold the window back.
const EFFECTS_FROM: f64 = 0.4;

const QUALITY: [u32; 3] = [0xf87171, 0xfbbf24, 0x34d399];

fn chrome(light: bool) -> LiquidMetalParams {
    let base = LiquidMetalParams {
        scale: 70.0,
        bevel: 5.0,
        dome: 0.75,
        iridescence: 0.22,
        flow: 0.7,
        speed: 0.3,
        ..LiquidMetalParams::chrome()
    };
    if light {
        LiquidMetalParams {
            contrast: 0.9,
            light: rgb(0xf4f8ff),
            ..base
        }
    } else {
        base
    }
}

fn mesh(dark: bool) -> MeshGradientParams {
    let colors = if dark {
        Stops::hex(&[0x0b1530, 0x0e7490, 0x4c1d95, 0x0891b2, 0x111827])
    } else {
        Stops::hex(&[0xe0f2fe, 0xa5f3fc, 0xddd6fe, 0xbae6fd, 0xf8fafc])
    };
    MeshGradientParams {
        colors,
        speed: 0.12,
        drift: 0.1,
        ..MeshGradientParams::dusk()
    }
}

fn layer(kind: u32, data: [f32; 4], shape: Shape, t: f32) -> Option<FxLayer> {
    let fx = match kind {
        CHROME => Fx::fill(chrome(data[0] > 0.5).anchor(FillAnchor::Element)),
        RIM => Fx::fill(
            chrome(false)
                .anchor(FillAnchor::Element)
                .border(data[0].max(1.0)),
        ),
        MESH => Fx::fill(mesh(data[0] > 0.5)),
        PULSE => Fx::pulse(PulseParams {
            style: PulseStyle::Radar,
            color: rgb(QUALITY[(data[0].clamp(0.0, 2.0)) as usize]),
            reach: 9.0,
            count: 2,
            period: 2.6,
            intensity: 0.9,
            ..PulseParams::default()
        }),
        SHIMMER => Fx::shimmer(ShimmerParams {
            intensity: if data[0] > 0.5 { 0.1 } else { 0.35 },
            period: 1.6,
            band: 60.0,
            ..ShimmerParams::default()
        }),
        SPARKLES => Fx::sparkles(SparkleParams {
            burst: Some(data[0]),
            count: 14,
            size: 9.0,
            color: rgb(0xa5f3fc),
            margin: 40.0,
            ..SparkleParams::default()
        }),
        _ => return None,
    };
    Some(fx.on(shape).at(t))
}

/// Draws the effect slots of every frame on top of Rune's own pass.
#[derive(Default)]
pub struct Effects {
    fx: Option<FxRenderer>,
    moving: bool,
}

impl Compositor<Ui> for Effects {
    fn animating(&self) -> bool {
        self.moving
    }

    fn render(&mut self, f: &mut Frame<'_, Ui>) {
        let (device, queue, size) = (f.device, f.queue, f.size);
        let t = f.rt.t;
        let ui = &f.rt.state;
        if !ui.settings.effects || t < EFFECTS_FROM {
            self.moving = ui.settings.effects;
            f.draw_ui();
            return;
        }
        self.moving = t - ui.stirred < AWAKE_SECONDS || ui.refreshing || ui.is_waiting();
        let marks = &f.rt.marks;
        let mut at: Vec<u32> = Vec::new();
        let mut layers: Vec<FxLayer> = Vec::new();
        let mut slots: Vec<FxSlot> = Vec::new();
        for (n, instance) in f.rt.instances.iter().enumerate() {
            if instance.params[1] != KIND_EFFECT {
                continue;
            }
            let kind = marks.get(n).copied().unwrap_or(0) & 0xff;
            let k = instance.params[3];
            let [x, y, w, h] = instance.rect.map(|v| v / k);
            let r = instance.radii.iter().fold(0.0f32, |a, v| a.max(v.abs())) / k;
            let Some(l) = layer(kind, instance.uv, Shape::rounded(x, y, w, h, r), t as f32) else {
                continue;
            };
            at.push(n as u32);
            layers.push(l);
            slots.push(
                FxSlot::default()
                    .scale(k)
                    .clip(instance.clip, instance.fade),
            );
        }
        let fx = self
            .fx
            .get_or_insert_with(|| FxRenderer::new(device, f.format, FxOptions { dither: true }));
        fx.set_srgb_blend(f.srgb_blend());
        fx.add_format(device, wgpu::TextureFormat::Rgba16Float);
        fx.prepare_with_slots(
            device,
            queue,
            &FxFrame {
                viewport: [size.0, size.1],
                scale: 1.0,
                layers: &layers,
                backdrop: None,
                sources: &[],
            },
            &slots,
        );
        let clear = f.rt.clear_color();
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, size);
        f.ui.draw_with_backdrop(
            f.encoder,
            f.target,
            clear,
            &mut |phase, s: &EffectSite<'_>| {
                let Some(k) = at.iter().position(|n| *n == s.index) else {
                    return;
                };
                match phase {
                    EffectPhase::Backdrop(encoder) => {
                        if let Some(backdrop) = s.backdrop {
                            fx.blur_backdrop(device, queue, encoder, backdrop, k);
                        }
                    }
                    EffectPhase::Draw(pass) => fx.draw_layer_in(pass, s.format, s.origin, k),
                }
            },
        );
    }
}
