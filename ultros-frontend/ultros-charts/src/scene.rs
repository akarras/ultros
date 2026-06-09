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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colors() {
        let c = Color::hex("#60a5fa");
        assert_eq!((c.r, c.g, c.b, c.a), (0x60, 0xa5, 0xfa, 1.0));
        assert_eq!(Color::rgb(1, 2, 3).with_alpha(0.5).a, 0.5);
    }
}
