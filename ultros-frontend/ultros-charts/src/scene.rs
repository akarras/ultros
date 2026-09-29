//! Renderer-agnostic display list.
//!
//! Chart layouts (`charts/`) build a [`Scene`]; backends consume it without
//! knowing anything about market data: `svg.rs` serializes it to an SVG
//! string for the server PNG pipeline, and the Leptos components (PR 2)
//! render it as reactive SVG nodes.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// Opacity in `0.0..=1.0`.
    pub a: f32,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub fn with_alpha(mut self, a: f32) -> Self {
        self.a = a;
        self
    }

    /// Parse a `#rrggbb` literal. Panics on malformed input; only ever
    /// called with compile-time constants from `Theme`.
    pub fn hex(hex: &str) -> Self {
        let hex = hex.trim_start_matches('#');
        assert!(hex.len() == 6, "expected #rrggbb, got {hex}");
        let parse = |range: std::ops::Range<usize>| {
            u8::from_str_radix(&hex[range], 16).expect("bad hex color")
        };
        Self::rgb(parse(0..2), parse(2..4), parse(4..6))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
    /// `(dash, gap)` lengths in px; `None` = solid.
    pub dash: Option<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAnchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        rx: f32,
        fill: Color,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        stroke: Stroke,
    },
    /// Open stroked polyline (price lines, sparklines).
    Polyline {
        points: Vec<(f32, f32)>,
        stroke: Stroke,
    },
    /// Filled area: `points` plus a closing run along `baseline_y`.
    Area {
        points: Vec<(f32, f32)>,
        baseline_y: f32,
        fill: Color,
    },
    /// Pre-serialized path data. Lets a layout emit N marks that share one
    /// fill or stroke as a single node instead of N nodes — the difference
    /// between 2,000 SVG elements and 1 for a dense chart.
    Path {
        d: String,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
        fill: Color,
    },
    /// `y` is the text baseline (no dominant-baseline games — resvg's
    /// support for it is spotty, so layouts compute baselines directly).
    Text {
        x: f32,
        y: f32,
        content: String,
        size: f32,
        color: Color,
        anchor: TextAnchor,
        bold: bool,
    },
    /// Embedded raster image as a data URI (item icons).
    Image {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        href: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    /// `None` = transparent (web; the page supplies the background).
    pub background: Option<Color>,
    pub font_family: String,
    pub nodes: Vec<Node>,
}

/// Rough advance width of `text` rendered at `size` px, used to right-align
/// text runs that the layout has to position by hand (there is no text
/// metrics engine here — the SVG backend hands the string to resvg and the
/// Leptos backend hands it to the browser).
///
/// **Why this is not `text.len()`.** `len()` is *bytes*. Every FFXIV world on
/// the Chinese and Korean data centres has a CJK name (紫水靈園, 카벙클), and
/// those are 3 bytes per character in UTF-8 — so a byte count overestimates a
/// CJK legend chip by ~3x while a per-character count underestimates it by
/// ~2x, because CJK glyphs are full-width. Split the difference: count
/// characters, and charge full-width ones the whole em.
///
/// The Latin factor (0.54) reproduces the 7px-per-char figure the legend used
/// at 13px, so Latin layouts are unchanged.
pub fn estimate_text_width(text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| if is_full_width(c) { size } else { size * 0.54 })
        .sum()
}

/// Whether `c` occupies a full em in a typical CJK font.
///
/// Deliberately a plain range check rather than a `unicode-width` dependency:
/// the only thing riding on it is a legend x-offset, and these ranges cover
/// everything ultros actually renders — Hangul, kana, CJK ideographs (plus the
/// Traditional-Chinese extension block), and the fullwidth forms that show up
/// in world names such as `Kuji／Sargatanas`.
fn is_full_width(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F     // Hangul Jamo
        | 0x2E80..=0x303E   // CJK radicals, kangxi, CJK symbols/punctuation
        | 0x3041..=0x33FF   // kana, Hangul compat jamo, CJK compat
        | 0x3400..=0x4DBF   // CJK ext A
        | 0x4E00..=0x9FFF   // CJK unified ideographs
        | 0xA960..=0xA97F   // Hangul Jamo extended-A
        | 0xAC00..=0xD7A3   // Hangul syllables
        | 0xF900..=0xFAFF   // CJK compatibility ideographs
        | 0xFE30..=0xFE4F   // CJK compatibility forms
        | 0xFF00..=0xFF60   // fullwidth forms
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x3FFFD // CJK ext B and beyond
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn assert_width(text: &str, expected: f32) {
        let got = estimate_text_width(text, 13.0);
        assert!(
            (got - expected).abs() < 0.01,
            "estimate_text_width({text:?}) = {got}, expected {expected}"
        );
    }

    #[test]
    fn latin_width_matches_the_legend_s_old_7px_per_char() {
        // The legend used `name.len() as f32 * 7.0` at size 13; keeping Latin
        // output within a rounding error of that is what makes this change
        // safe for every existing chart.
        assert_width("Sargatanas", 70.2);
    }

    #[test]
    fn cjk_world_names_are_measured_per_character_not_per_byte() {
        // 紫水靈園 is 4 characters but 12 UTF-8 bytes, so the old byte count
        // charged the legend 12 * 7 = 84px for a chip that is really ~52px
        // wide — pushing the whole legend row left off its right edge.
        assert_eq!("紫水靈園".len(), 12);
        assert_width("紫水靈園", 52.0);
        // Hangul syllables are full-width too (카벙클 = a Korean world name).
        assert_width("카벙클", 39.0);
        // Fullwidth solidus, as seen in `Kuji／Sargatanas`.
        assert_width("／", 13.0);
        // Mixed runs charge each script its own width.
        assert_width("Kuji／Sargatanas", 14.0 * 7.02 + 13.0);
    }

    #[test]
    fn parses_hex_colors() {
        let c = Color::hex("#60a5fa");
        assert_eq!((c.r, c.g, c.b, c.a), (0x60, 0xa5, 0xfa, 1.0));
        assert_eq!(Color::rgb(1, 2, 3).with_alpha(0.5).a, 0.5);
    }
}
