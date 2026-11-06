use rune::{Color, oklch, oklcha};

/// The window's colours in one appearance.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub backdrop: Color,
    pub surface: Color,
    pub raised: Color,
    pub border: Color,
    pub hairline: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub on_accent: Color,
    pub good: Color,
    pub fair: Color,
    pub poor: Color,
    pub track: Color,
    pub hover: Color,
    pub shadow: Color,
    pub code: Color,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            dark,
            backdrop: oklch(0.155, 0.012, 275.0),
            surface: oklcha(0.235, 0.014, 275.0, 0.82),
            raised: oklch(0.27, 0.016, 275.0),
            border: oklcha(1.0, 0.0, 0.0, 0.085),
            hairline: oklcha(1.0, 0.0, 0.0, 0.06),
            text: oklch(0.965, 0.004, 275.0),
            muted: oklch(0.74, 0.012, 275.0),
            faint: oklch(0.58, 0.014, 275.0),
            accent: oklch(0.78, 0.13, 205.0),
            accent_soft: oklcha(0.78, 0.13, 205.0, 0.16),
            on_accent: oklch(0.18, 0.03, 220.0),
            good: oklch(0.8, 0.17, 152.0),
            fair: oklch(0.83, 0.15, 80.0),
            poor: oklch(0.7, 0.19, 25.0),
            track: oklcha(1.0, 0.0, 0.0, 0.08),
            hover: oklcha(1.0, 0.0, 0.0, 0.045),
            shadow: oklcha(0.0, 0.0, 0.0, 0.45),
            code: oklcha(0.0, 0.0, 0.0, 0.28),
        }
    } else {
        Palette {
            dark,
            backdrop: oklch(0.965, 0.008, 265.0),
            surface: oklcha(1.0, 0.0, 0.0, 0.86),
            raised: oklch(1.0, 0.0, 0.0),
            border: oklcha(0.25, 0.03, 265.0, 0.1),
            hairline: oklcha(0.25, 0.03, 265.0, 0.08),
            text: oklch(0.22, 0.02, 265.0),
            muted: oklch(0.46, 0.02, 265.0),
            faint: oklch(0.6, 0.018, 265.0),
            accent: oklch(0.56, 0.14, 225.0),
            accent_soft: oklcha(0.56, 0.14, 225.0, 0.11),
            on_accent: oklch(0.99, 0.0, 0.0),
            good: oklch(0.6, 0.15, 152.0),
            fair: oklch(0.66, 0.14, 70.0),
            poor: oklch(0.58, 0.2, 27.0),
            track: oklcha(0.25, 0.03, 265.0, 0.09),
            hover: oklcha(0.25, 0.03, 265.0, 0.05),
            shadow: oklcha(0.25, 0.05, 265.0, 0.16),
            code: oklcha(0.25, 0.03, 265.0, 0.06),
        }
    }
}
