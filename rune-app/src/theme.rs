//! Direct port of the old CSS design tokens (`src/index.css` at c4e8721).
//! Rune stores colours in linear sRGB, so authored hex values are decoded.

use rune::Color;
use rune::theme::srgb_decode;

pub const THEMES: [&str; 5] = ["ultraviolet", "astro", "carbon", "emerald", "bloodmoon"];

pub fn hex(value: &str) -> Color {
    let v = value.trim_start_matches('#');
    let byte = |a: usize, b: usize| -> f32 {
        u8::from_str_radix(&v[a..b], 16).unwrap_or(0) as f32 / 255.0
    };
    Color {
        r: srgb_decode(byte(0, 2)),
        g: srgb_decode(byte(2, 4)),
        b: srgb_decode(byte(4, 6)),
        a: 1.0,
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub primary: Color,
    pub on_primary: Color,
    pub bg: Color,
    pub text: Color,
    pub muted: Color,
    pub subtle: Color,
    pub surface: Color,
    pub elevated: Color,
    pub input: Color,
    pub border: Color,
    pub border_subtle: Color,
    pub glass: Color,
    pub glass_border: Color,
    pub success: Color,
    pub danger: Color,
    pub warning: Color,
}

impl Palette {
    pub fn new(theme: &str, dark: bool) -> Self {
        let (primary, on_primary, dark_bg, light_bg) = match theme {
            "astro" => (hex("3b82f6"), hex("ffffff"), hex("06070c"), hex("eef4ff")),
            "carbon" if dark => (hex("ffffff"), hex("09090b"), hex("0a0a0a"), hex("f5f5f4")),
            "carbon" => (hex("1c1917"), hex("fafaf9"), hex("0a0a0a"), hex("f5f5f4")),
            "emerald" => (hex("10b981"), hex("ffffff"), hex("040a08"), hex("ecfdf5")),
            "bloodmoon" => (hex("ef4444"), hex("ffffff"), hex("0a0404"), hex("fff1f2")),
            _ => (hex("8b5cf6"), hex("ffffff"), hex("08080a"), hex("f5f1ff")),
        };
        if dark {
            Self {
                primary,
                on_primary,
                bg: dark_bg,
                text: hex("e4e4e7"),
                muted: hex("b4b4bd"),
                subtle: hex("8b8b96"),
                surface: hex("141417"),
                elevated: hex("0d0d10"),
                input: hex("2b2b32"),
                border: hex("26262c"),
                border_subtle: hex("45454f"),
                glass: hex("18181b").alpha(0.6),
                glass_border: hex("ffffff").alpha(0.05),
                success: hex("10b981"),
                danger: hex("ef4444"),
                warning: hex("f59e0b"),
            }
        } else {
            Self {
                primary,
                on_primary,
                bg: light_bg,
                text: hex("1c1917"),
                muted: hex("57534e"),
                subtle: hex("78716c"),
                surface: hex("fafaf9"),
                elevated: hex("f5f5f4"),
                input: hex("e7e5e4"),
                border: hex("d6d3d1"),
                border_subtle: hex("e7e5e4"),
                glass: hex("fafaf9").alpha(0.78),
                glass_border: hex("1c1917").alpha(0.12),
                success: hex("10b981"),
                danger: hex("ef4444"),
                warning: hex("f59e0b"),
            }
        }
    }
}
