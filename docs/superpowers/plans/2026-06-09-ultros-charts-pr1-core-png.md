# ultros_charts PR 1 — Scene-Graph Core + PNG Path Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite `ultros-frontend/ultros-charts` as a scene-graph chart crate (pure math + display list + SVG serializer), swap the Discord/item-card PNG endpoint onto it, and remove plotters from the workspace.

**Architecture:** Chart layouts build a renderer-agnostic `Scene` (rects/lines/polylines/areas/circles/text/images). `svg::scene_to_svg` serializes it to an SVG string; the server rasterizes that with the already-present resvg. PR 2 will add a Leptos renderer over the same scenes; PR 3 sparklines. Spec: `docs/superpowers/specs/2026-06-09-ultros-charts-design.md`.

**Tech Stack:** Rust (edition 2024), chrono, itertools, base64, image (feature-gated), resvg (stays in `ultros`).

---

## Context for the implementer

- Workspace root: `C:\Users\chw11\code\ultros`. Work happens in two crates: `ultros-frontend/ultros-charts` (the rewrite) and `ultros` (the only consumer, `ultros/src/web/item_card.rs`).
- `ultros-app` declares `ultros-charts` as a dependency but imports nothing from it. **Leave that dep line alone** — PR 2 uses it.
- **Strategy is additive:** new modules land beside the old plotters code, so every commit keeps the workspace compiling. The old code and the plotters deps are deleted in Task 11.
- Test with `cargo test -p ultros-charts` per task (fast — the crate has no heavy deps). The full `./check_ci.sh` runs once at the end (Task 12); it needs the git submodules initialized (see CLAUDE.md).
- No leptos-i18n work in this PR: the only new user-visible string ("No recent sales") is rendered *inside the PNG*, like the existing "Sale History" caption — server-rendered image text is out of i18n scope.
- Commit messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

### Task 1: Branch, scene primitives, theme

**Files:**
- Create: `ultros-frontend/ultros-charts/src/scene.rs`
- Create: `ultros-frontend/ultros-charts/src/theme.rs`
- Modify: `ultros-frontend/ultros-charts/src/lib.rs` (add module declarations at the top; leave all existing code in place)

- [ ] **Step 1: Create the branch**

```bash
git checkout -b ultros-charts-rewrite
```

- [ ] **Step 2: Write the failing test**

Create `ultros-frontend/ultros-charts/src/scene.rs` containing ONLY the test module for now:

```rust
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
```

Add to the top of `ultros-frontend/ultros-charts/src/lib.rs` (above the existing `use` lines):

```rust
pub mod scene;
pub mod theme;
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p ultros-charts scene`
Expected: FAIL to compile — `Color` not found.

- [ ] **Step 4: Write the implementation**

Prepend to `scene.rs` (above the test module):

```rust
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
```

Create `ultros-frontend/ultros-charts/src/theme.rs`:

```rust
use crate::scene::Color;

/// The shared category palette — same hexes the web UI uses today
/// (`CATEGORY_PALETTE` in price_history_chart.rs).
pub const CATEGORY_PALETTE: [&str; 12] = [
    "#60a5fa", "#f97316", "#34d399", "#a78bfa", "#fb7185", "#facc15", "#22d3ee", "#c084fc",
    "#4ade80", "#f472b6", "#94a3b8", "#fdba74",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// `None` = transparent (web; the page supplies the background).
    pub background: Option<Color>,
    pub text: Color,
    pub text_muted: Color,
    pub grid: Color,
    /// Per-series colors, cycled if there are more series than entries.
    pub palette: Vec<Color>,
    pub volume: Color,
    pub market_average: Color,
    pub trend: Color,
    pub font_family: String,
}

impl Theme {
    fn base(background: Option<Color>) -> Self {
        Self {
            background,
            text: Color::hex("#e5e7eb"),
            text_muted: Color::hex("#9ca3af"),
            grid: Color::hex("#9ca3af").with_alpha(0.15),
            palette: CATEGORY_PALETTE.iter().map(|c| Color::hex(c)).collect(),
            volume: Color::hex("#22c55e"),
            market_average: Color::hex("#facc15"),
            trend: Color::hex("#94a3b8"),
            font_family: "Jaldi, sans-serif".to_string(),
        }
    }

    /// Dark card for PNG output (Discord embeds, the /item/{world}/{id} card).
    pub fn dark_card() -> Self {
        Self::base(Some(Color::hex("#202124")))
    }

    /// Transparent-background variant for the web UI (PR 2).
    pub fn site() -> Self {
        Self::base(None)
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p ultros-charts scene`
Expected: PASS (1 test)

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-charts/src/scene.rs ultros-frontend/ultros-charts/src/theme.rs ultros-frontend/ultros-charts/src/lib.rs
git commit -m "feat(charts): scene-graph display list and theme" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 2: SVG serializer

**Files:**
- Create: `ultros-frontend/ultros-charts/src/svg.rs`
- Modify: `ultros-frontend/ultros-charts/src/lib.rs` (add `pub mod svg;`)

- [ ] **Step 1: Write the failing test**

Create `svg.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Color, Node, Scene, Stroke, TextAnchor};

    #[test]
    fn serializes_each_node_kind() {
        let scene = Scene {
            width: 100.0,
            height: 50.0,
            background: Some(Color::hex("#202124")),
            font_family: "Jaldi, sans-serif".to_string(),
            nodes: vec![
                Node::Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 3.0,
                    height: 4.0,
                    rx: 1.0,
                    fill: Color::rgb(255, 0, 0).with_alpha(0.5),
                },
                Node::Line {
                    x1: 0.0,
                    y1: 0.0,
                    x2: 10.0,
                    y2: 10.0,
                    stroke: Stroke {
                        color: Color::rgb(0, 255, 0),
                        width: 1.5,
                        dash: Some((2.0, 4.0)),
                    },
                },
                Node::Polyline {
                    points: vec![(0.0, 0.0), (5.0, 5.0)],
                    stroke: Stroke {
                        color: Color::rgb(0, 0, 255),
                        width: 2.0,
                        dash: None,
                    },
                },
                Node::Area {
                    points: vec![(0.0, 10.0), (5.0, 5.0)],
                    baseline_y: 20.0,
                    fill: Color::rgb(1, 2, 3),
                },
                Node::Circle {
                    cx: 5.0,
                    cy: 5.0,
                    r: 2.0,
                    fill: Color::rgb(9, 9, 9),
                },
                Node::Text {
                    x: 1.0,
                    y: 1.0,
                    content: "a < b".to_string(),
                    size: 13.0,
                    color: Color::rgb(0, 0, 0),
                    anchor: TextAnchor::Middle,
                    bold: true,
                },
                Node::Image {
                    x: 0.0,
                    y: 0.0,
                    width: 8.0,
                    height: 8.0,
                    href: "data:image/png;base64,AAAA".to_string(),
                },
            ],
        };
        let svg = scene_to_svg(&scene);
        assert!(svg.starts_with("<svg "));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains(r#"viewBox="0 0 100 50""#));
        assert!(svg.contains(r##"fill="#202124""##)); // background rect
        assert!(svg.contains(r##"fill="#ff0000" fill-opacity="0.500""##));
        assert!(svg.contains(r#"stroke-dasharray="2.0 4.0""#));
        assert!(svg.contains(r##"<polyline points="0.0,0.0 5.0,5.0" fill="none""##));
        assert!(svg.contains(r##"Z" fill="#010203""##)); // area path closes to baseline
        assert!(svg.contains(r#"<circle cx="5.0" cy="5.0" r="2.0""#));
        assert!(svg.contains("a &lt; b"));
        assert!(svg.contains(r#"text-anchor="middle""#));
        assert!(svg.contains(r#"font-weight="bold""#));
        assert!(svg.contains(r#"xlink:href="data:image/png;base64,AAAA""#));
    }
}
```

Add `pub mod svg;` to `lib.rs`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros-charts svg`
Expected: FAIL to compile — `scene_to_svg` not found.

- [ ] **Step 3: Write the implementation**

Prepend to `svg.rs`:

```rust
//! Serialize a [`Scene`] to an SVG string.
//!
//! The server rasterizes this with resvg, so stick to plain SVG 1.1 that
//! usvg supports: no CSS classes, no `rgba()` colors (use `*-opacity`
//! attributes), `xlink:href` for images.

use std::fmt::Write;

use crate::scene::{Color, Node, Scene, Stroke, TextAnchor};

fn hex(c: &Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn push_fill(out: &mut String, c: &Color) {
    let _ = write!(out, r#" fill="{}""#, hex(c));
    if c.a < 1.0 {
        let _ = write!(out, r#" fill-opacity="{:.3}""#, c.a);
    }
}

fn push_stroke(out: &mut String, s: &Stroke) {
    let _ = write!(
        out,
        r#" stroke="{}" stroke-width="{:.2}" stroke-linecap="round" stroke-linejoin="round""#,
        hex(&s.color),
        s.width
    );
    if s.color.a < 1.0 {
        let _ = write!(out, r#" stroke-opacity="{:.3}""#, s.color.a);
    }
    if let Some((dash, gap)) = s.dash {
        let _ = write!(out, r#" stroke-dasharray="{dash:.1} {gap:.1}""#);
    }
}

fn points_attr(points: &[(f32, f32)]) -> String {
    let mut out = String::with_capacity(points.len() * 12);
    for (i, (x, y)) in points.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let _ = write!(out, "{x:.1},{y:.1}");
    }
    out
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn scene_to_svg(scene: &Scene) -> String {
    let mut out = String::with_capacity(scene.nodes.len() * 96 + 256);
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" font-family="{font}">"#,
        w = scene.width,
        h = scene.height,
        font = escape_text(&scene.font_family),
    );
    if let Some(bg) = &scene.background {
        let _ = write!(
            out,
            r#"<rect x="0" y="0" width="{:.0}" height="{:.0}""#,
            scene.width, scene.height
        );
        push_fill(&mut out, bg);
        out.push_str("/>");
    }
    for node in &scene.nodes {
        match node {
            Node::Rect {
                x,
                y,
                width,
                height,
                rx,
                fill,
            } => {
                let _ = write!(
                    out,
                    r#"<rect x="{x:.1}" y="{y:.1}" width="{width:.1}" height="{height:.1}""#
                );
                if *rx > 0.0 {
                    let _ = write!(out, r#" rx="{rx:.1}""#);
                }
                push_fill(&mut out, fill);
                out.push_str("/>");
            }
            Node::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
            } => {
                let _ = write!(
                    out,
                    r#"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}""#
                );
                push_stroke(&mut out, stroke);
                out.push_str("/>");
            }
            Node::Polyline { points, stroke } => {
                let _ = write!(
                    out,
                    r#"<polyline points="{}" fill="none""#,
                    points_attr(points)
                );
                push_stroke(&mut out, stroke);
                out.push_str("/>");
            }
            Node::Area {
                points,
                baseline_y,
                fill,
            } => {
                if points.len() < 2 {
                    continue;
                }
                let mut d = String::new();
                for (i, (x, y)) in points.iter().enumerate() {
                    let _ = write!(d, "{}{x:.1} {y:.1}", if i == 0 { "M" } else { "L" });
                }
                let first_x = points[0].0;
                let last_x = points[points.len() - 1].0;
                let _ = write!(
                    d,
                    "L{last_x:.1} {baseline_y:.1}L{first_x:.1} {baseline_y:.1}Z"
                );
                let _ = write!(out, r#"<path d="{d}""#);
                push_fill(&mut out, fill);
                out.push_str("/>");
            }
            Node::Circle { cx, cy, r, fill } => {
                let _ = write!(out, r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{r:.1}""#);
                push_fill(&mut out, fill);
                out.push_str("/>");
            }
            Node::Text {
                x,
                y,
                content,
                size,
                color,
                anchor,
                bold,
            } => {
                let anchor = match anchor {
                    TextAnchor::Start => "start",
                    TextAnchor::Middle => "middle",
                    TextAnchor::End => "end",
                };
                let _ = write!(
                    out,
                    r#"<text x="{x:.1}" y="{y:.1}" font-size="{size:.1}" text-anchor="{anchor}""#
                );
                if *bold {
                    out.push_str(r#" font-weight="bold""#);
                }
                push_fill(&mut out, color);
                let _ = write!(out, ">{}</text>", escape_text(content));
            }
            Node::Image {
                x,
                y,
                width,
                height,
                href,
            } => {
                let _ = write!(
                    out,
                    r#"<image x="{x:.1}" y="{y:.1}" width="{width:.1}" height="{height:.1}" xlink:href="{href}"/>"#
                );
            }
        }
    }
    out.push_str("</svg>");
    out
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ultros-charts svg`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-charts/src/svg.rs ultros-frontend/ultros-charts/src/lib.rs
git commit -m "feat(charts): SVG serializer for scenes" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 3: Scales and tick generation

**Files:**
- Create: `ultros-frontend/ultros-charts/src/scale.rs`
- Modify: `ultros-frontend/ultros-charts/src/lib.rs` (add `pub mod scale;`)

- [ ] **Step 1: Write the failing tests**

Create `scale.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn ts(secs: i64) -> NaiveDateTime {
        chrono::DateTime::from_timestamp(secs, 0).unwrap().naive_utc()
    }

    #[test]
    fn short_number_formats_like_the_web_ui() {
        assert_eq!(short_number(0), "0");
        assert_eq!(short_number(999), "999");
        assert_eq!(short_number(1000), "1.00K");
        assert_eq!(short_number(1500), "1.50K");
        assert_eq!(short_number(999999), "1000.00K");
        assert_eq!(short_number(1000000), "1.00mil");
        assert_eq!(short_number(1500000), "1.50mil");
    }

    #[test]
    fn linear_scale_maps_and_inverts_range() {
        let s = LinearScale::new((0.0, 100.0), (200.0, 0.0));
        assert_eq!(s.scale(0.0), 200.0);
        assert_eq!(s.scale(100.0), 0.0);
        assert_eq!(s.scale(50.0), 100.0);
    }

    #[test]
    fn linear_ticks_are_nice() {
        let s = LinearScale::new((0.0, 1000.0), (0.0, 1.0));
        assert_eq!(s.ticks(5), vec![0.0, 200.0, 400.0, 600.0, 800.0, 1000.0]);
    }

    #[test]
    fn degenerate_domain_widens() {
        let s = LinearScale::new((5.0, 5.0), (0.0, 10.0));
        assert_eq!(s.scale(5.0), 5.0);
    }

    #[test]
    fn time_ticks_pick_sensible_steps() {
        let scale = TimeScale::new(ts(1_700_000_000), ts(1_700_000_000 + 2 * 3600), (0.0, 100.0));
        let ticks = scale.ticks(6);
        assert!(!ticks.is_empty() && ticks.len() <= 6);
        // Sub-day spans label as %H:%M
        assert!(ticks[0].label.contains(':'));
    }

    #[test]
    fn equal_timestamps_widen_30_minutes() {
        let t = ts(1_700_000_000);
        let scale = TimeScale::new(t, t, (0.0, 100.0));
        assert_eq!(scale.scale(t), 50.0);
    }
}
```

Add `pub mod scale;` to `lib.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-charts scale`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation**

Prepend to `scale.rs`:

```rust
//! Numeric and time axes: domain→pixel mapping, "nice" tick generation,
//! and the K/mil number formatting shared with the web UI.

use chrono::NaiveDateTime;

/// Format an integer gil value as `1.50K` / `2.30mil`, matching the web UI.
pub fn short_number(value: i32) -> String {
    match value {
        1_000_000.. => format!("{:.2}mil", value as f32 / 1_000_000.0),
        1_000..=999_999 => format!("{:.2}K", value as f32 / 1_000.0),
        _ => value.to_string(),
    }
}

/// Maps a numeric domain onto a pixel range. The range may be inverted
/// (`range.0 > range.1`) — SVG y grows downward, so price scales pass
/// `(bottom, top)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearScale {
    domain: (f64, f64),
    range: (f32, f32),
}

impl LinearScale {
    pub fn new(domain: (f64, f64), range: (f32, f32)) -> Self {
        // Degenerate domains (single price) get widened so scale() stays finite.
        let domain = if domain.0 == domain.1 {
            (domain.0 - 1.0, domain.1 + 1.0)
        } else {
            domain
        };
        Self { domain, range }
    }

    pub fn scale(&self, v: f64) -> f32 {
        let t = (v - self.domain.0) / (self.domain.1 - self.domain.0);
        self.range.0 + t as f32 * (self.range.1 - self.range.0)
    }

    /// Tick values at "nice" 1/2/5×10ⁿ steps, clamped inside the domain.
    pub fn ticks(&self, target: usize) -> Vec<f64> {
        let span = self.domain.1 - self.domain.0;
        if span <= 0.0 || target == 0 {
            return Vec::new();
        }
        let raw_step = span / target as f64;
        let magnitude = 10f64.powf(raw_step.log10().floor());
        let normalized = raw_step / magnitude;
        let step = magnitude
            * if normalized <= 1.0 {
                1.0
            } else if normalized <= 2.0 {
                2.0
            } else if normalized <= 5.0 {
                5.0
            } else {
                10.0
            };
        let mut v = (self.domain.0 / step).ceil() * step;
        let mut out = Vec::new();
        while v <= self.domain.1 + step * 1e-9 {
            out.push(v);
            v += step;
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimeTick {
    pub ts: NaiveDateTime,
    pub label: String,
}

/// Maps naive-UTC timestamps onto a pixel range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeScale {
    start: i64,
    end: i64,
    range: (f32, f32),
}

const MINUTE: i64 = 60;
const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;

/// Candidate tick steps, smallest first.
const TIME_STEPS: [i64; 15] = [
    MINUTE,
    5 * MINUTE,
    15 * MINUTE,
    30 * MINUTE,
    HOUR,
    3 * HOUR,
    6 * HOUR,
    12 * HOUR,
    DAY,
    2 * DAY,
    7 * DAY,
    14 * DAY,
    30 * DAY,
    90 * DAY,
    365 * DAY,
];

impl TimeScale {
    pub fn new(start: NaiveDateTime, end: NaiveDateTime, range: (f32, f32)) -> Self {
        let (mut start, mut end) = (start.and_utc().timestamp(), end.and_utc().timestamp());
        if start == end {
            // Single-instant data: widen ±30 min, same as the old plotters chart.
            start -= 30 * MINUTE;
            end += 30 * MINUTE;
        }
        Self { start, end, range }
    }

    pub fn scale(&self, ts: NaiveDateTime) -> f32 {
        let t = (ts.and_utc().timestamp() - self.start) as f64 / (self.end - self.start) as f64;
        self.range.0 + t as f32 * (self.range.1 - self.range.0)
    }

    /// At most `target` ticks aligned to step boundaries, labelled with a
    /// format that matches the step size.
    pub fn ticks(&self, target: usize) -> Vec<TimeTick> {
        let span = self.end - self.start;
        let step = TIME_STEPS
            .iter()
            .copied()
            .find(|step| span / step <= target as i64)
            .unwrap_or(365 * DAY);
        let format = if step < HOUR {
            "%H:%M"
        } else if step < DAY {
            "%m-%d %H:%M"
        } else if step < 30 * DAY {
            "%m-%d"
        } else {
            "%Y-%m"
        };
        let mut tick = self.start.div_euclid(step) * step;
        if tick < self.start {
            tick += step;
        }
        let mut out = Vec::new();
        while tick <= self.end {
            if let Some(ts) = chrono::DateTime::from_timestamp(tick, 0) {
                let ts = ts.naive_utc();
                out.push(TimeTick {
                    ts,
                    label: ts.format(format).to_string(),
                });
            }
            tick += step;
        }
        out
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-charts scale`
Expected: PASS (6 tests)

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-charts/src/scale.rs ultros-frontend/ultros-charts/src/lib.rs
git commit -m "feat(charts): linear and time scales with nice ticks" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 4: Test fixtures + outlier filtering

**Files:**
- Create: `ultros-frontend/ultros-charts/src/test_util.rs`
- Create: `ultros-frontend/ultros-charts/src/data/mod.rs`
- Create: `ultros-frontend/ultros-charts/src/data/outliers.rs`
- Modify: `ultros-frontend/ultros-charts/src/lib.rs`

- [ ] **Step 1: Write the fixtures and failing tests**

Create `test_util.rs`:

```rust
//! Shared fixtures for unit tests: a synthetic world tree and SaleHistory rows.

use chrono::NaiveDateTime;
use ultros_api_types::world::{Datacenter, Region, World, WorldData};
use ultros_api_types::world_helper::WorldHelper;
use ultros_api_types::SaleHistory;

pub(crate) fn ts(secs: i64) -> NaiveDateTime {
    chrono::DateTime::from_timestamp(secs, 0).unwrap().naive_utc()
}

pub(crate) fn sale(price: i32, quantity: i32, world_id: i32, sold: NaiveDateTime) -> SaleHistory {
    SaleHistory {
        id: 0,
        quantity,
        price_per_item: price,
        buying_character_id: 0,
        hq: false,
        sold_item_id: 1,
        sold_date: sold,
        world_id,
        buyer_name: None,
    }
}

/// Two regions; region 1 has two datacenters; datacenter 1 has two worlds.
/// World ids: 1 = Gilgamesh (Aether), 2 = Adamantoise (Aether),
/// 3 = Behemoth (Primal), 4 = Cerberus (Chaos / Europe).
pub(crate) fn world_helper() -> WorldHelper {
    WorldHelper::new(WorldData {
        regions: vec![
            Region {
                id: 1,
                name: "North-America".to_string(),
                datacenters: vec![
                    Datacenter {
                        id: 1,
                        name: "Aether".to_string(),
                        region_id: 1,
                        worlds: vec![
                            World {
                                id: 1,
                                name: "Gilgamesh".to_string(),
                                datacenter_id: 1,
                            },
                            World {
                                id: 2,
                                name: "Adamantoise".to_string(),
                                datacenter_id: 1,
                            },
                        ],
                    },
                    Datacenter {
                        id: 2,
                        name: "Primal".to_string(),
                        region_id: 1,
                        worlds: vec![World {
                            id: 3,
                            name: "Behemoth".to_string(),
                            datacenter_id: 2,
                        }],
                    },
                ],
            },
            Region {
                id: 2,
                name: "Europe".to_string(),
                datacenters: vec![Datacenter {
                    id: 3,
                    name: "Chaos".to_string(),
                    region_id: 2,
                    worlds: vec![World {
                        id: 4,
                        name: "Cerberus".to_string(),
                        datacenter_id: 3,
                    }],
                }],
            },
        ],
    })
}
```

(If the `world` module path differs, check `ultros-api-types/src/lib.rs` for the re-export — the structs are defined in `ultros-api-types/src/world.rs`.)

Create `data/mod.rs`:

```rust
pub mod outliers;
```

Create `data/outliers.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{sale, ts};

    #[test]
    fn small_samples_are_not_filtered() {
        let sales: Vec<_> = (0..5).map(|i| sale(100 + i, 1, 1, ts(0))).collect();
        assert!(iqr_bounds(&sales).is_none());
        assert_eq!(filter_outliers(&sales).len(), 5);
    }

    #[test]
    fn extreme_prices_are_filtered() {
        let mut sales: Vec<_> = (0..20).map(|i| sale(1000 + i, 1, 1, ts(i as i64))).collect();
        sales.push(sale(1_000_000, 1, 1, ts(21)));
        let filtered = filter_outliers(&sales);
        assert_eq!(filtered.len(), 20);
        assert!(filtered.iter().all(|s| s.price_per_item < 10_000));
    }
}
```

Add to `lib.rs`:

```rust
pub mod data;

#[cfg(test)]
pub(crate) mod test_util;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-charts outliers`
Expected: FAIL to compile — `iqr_bounds` / `filter_outliers` not found.

- [ ] **Step 3: Write the implementation**

Prepend to `data/outliers.rs`:

```rust
//! IQR-based outlier filtering — the same rule the old plotters chart and
//! the web UI used, consolidated here.

use std::borrow::Cow;

use itertools::Itertools;
use ultros_api_types::SaleHistory;

/// Outlier bounds: `(Q1 - 2.5*IQR, Q3 + 2.5*IQR)`. Returns `None` for
/// samples smaller than 10 — too little data to call anything an outlier.
pub fn iqr_bounds(sales: &[SaleHistory]) -> Option<(i32, i32)> {
    if sales.len() < 10 {
        return None;
    }
    let prices = sales
        .iter()
        .map(|s| s.price_per_item)
        .sorted()
        .collect::<Vec<_>>();
    let q1_index = prices.len() / 4;
    let q3_index = prices.len() - q1_index;
    let q1 = *prices.get(q1_index)?;
    let q3 = *prices.get(q3_index)?;
    let widened = ((q3 - q1) as f32 * 2.5) as i32;
    Some((q1 - widened, q3 + widened))
}

pub fn filter_outliers(sales: &[SaleHistory]) -> Cow<'_, [SaleHistory]> {
    match iqr_bounds(sales) {
        Some((min, max)) => Cow::Owned(
            sales
                .iter()
                .filter(|s| (min..=max).contains(&s.price_per_item))
                .cloned()
                .collect(),
        ),
        None => Cow::Borrowed(sales),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-charts outliers`
Expected: PASS (2 tests)

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-charts/src/test_util.rs ultros-frontend/ultros-charts/src/data ultros-frontend/ultros-charts/src/lib.rs
git commit -m "feat(charts): IQR outlier filter and test fixtures" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 5: World/DC/region grouping

**Files:**
- Create: `ultros-frontend/ultros-charts/src/data/grouping.rs`
- Modify: `ultros-frontend/ultros-charts/src/data/mod.rs` (add `pub mod grouping;`)

- [ ] **Step 1: Write the failing tests**

Create `data/grouping.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{sale, ts, world_helper};

    fn names(series: &[Series]) -> Vec<&str> {
        series.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn single_datacenter_groups_by_world() {
        let sales = vec![sale(100, 1, 1, ts(0)), sale(200, 1, 2, ts(10))];
        let series = group_sales_by_scope(&world_helper(), &sales);
        assert_eq!(names(&series), vec!["Adamantoise", "Gilgamesh"]);
    }

    #[test]
    fn single_region_groups_by_datacenter() {
        let sales = vec![sale(100, 1, 1, ts(0)), sale(200, 1, 3, ts(10))];
        let series = group_sales_by_scope(&world_helper(), &sales);
        assert_eq!(names(&series), vec!["Aether", "Primal"]);
    }

    #[test]
    fn multiple_regions_group_by_region() {
        let sales = vec![sale(100, 1, 1, ts(0)), sale(200, 1, 4, ts(10))];
        let series = group_sales_by_scope(&world_helper(), &sales);
        assert_eq!(names(&series), vec!["Europe", "North-America"]);
    }

    #[test]
    fn points_are_sorted_by_time() {
        let sales = vec![sale(100, 1, 1, ts(100)), sale(200, 1, 1, ts(50))];
        let series = group_sales_by_scope(&world_helper(), &sales);
        assert_eq!(series.len(), 1);
        assert_eq!(
            series[0].points.iter().map(|p| p.ts).collect::<Vec<_>>(),
            vec![ts(50), ts(100)]
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-charts grouping`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation**

Prepend to `data/grouping.rs`:

```rust
//! Group sales by the narrowest world-hierarchy level that still yields
//! multiple groups — ported from the old plotters chart. One deliberate
//! change: timestamps stay naive-UTC (the old code converted to the
//! server's local timezone, which is UTC in prod anyway; keeping UTC makes
//! output deterministic across environments).

use std::collections::HashSet;

use chrono::NaiveDateTime;
use itertools::Itertools;
use ultros_api_types::world_helper::{AnySelector, WorldHelper};
use ultros_api_types::SaleHistory;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SalePoint {
    /// Naive UTC, matching `SaleHistory::sold_date`.
    pub ts: NaiveDateTime,
    pub price: i32,
    pub quantity: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub name: String,
    /// Sorted by timestamp ascending.
    pub points: Vec<SalePoint>,
}

/// All sales on one datacenter → one series per world; one region → per
/// datacenter; otherwise per region. Series sort by name for stable colors.
pub fn group_sales_by_scope(world_helper: &WorldHelper, sales: &[SaleHistory]) -> Vec<Series> {
    let world_ids: HashSet<_> = sales
        .iter()
        .map(|s| AnySelector::World(s.world_id))
        .collect();
    let datacenters: HashSet<_> = world_ids
        .iter()
        .flat_map(|world| {
            world_helper
                .lookup_selector(*world)
                .and_then(|s| s.as_world())
                .map(|w| AnySelector::Datacenter(w.datacenter_id))
        })
        .collect();
    let regions: HashSet<_> = datacenters
        .iter()
        .flat_map(|dc| {
            world_helper
                .lookup_selector(*dc)
                .and_then(|dc| dc.as_datacenter())
                .map(|dc| AnySelector::Region(dc.region_id))
        })
        .collect();
    let selectors = if datacenters.len() == 1 {
        world_ids
    } else if regions.len() == 1 {
        datacenters
    } else {
        regions
    };
    selectors
        .into_iter()
        .flat_map(|selector| series_for(world_helper, selector, sales))
        .sorted_by_cached_key(|series| series.name.clone())
        .collect()
}

fn series_for(
    world_helper: &WorldHelper,
    selector: AnySelector,
    sales: &[SaleHistory],
) -> Option<Series> {
    let result = world_helper.lookup_selector(selector)?;
    let mut points: Vec<SalePoint> = sales
        .iter()
        .filter(|sale| {
            world_helper
                .lookup_selector(AnySelector::World(sale.world_id))
                .map(|world| world.is_in(&result))
                .unwrap_or_default()
        })
        .map(|sale| SalePoint {
            ts: sale.sold_date,
            price: sale.price_per_item,
            quantity: sale.quantity,
        })
        .collect();
    points.sort_by_key(|p| p.ts);
    Some(Series {
        name: result.get_name().to_string(),
        points,
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-charts grouping`
Expected: PASS (4 tests)

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-charts/src/data
git commit -m "feat(charts): scope-based series grouping" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 6: VWAP and volume bucketing

**Files:**
- Create: `ultros-frontend/ultros-charts/src/data/buckets.rs`
- Modify: `ultros-frontend/ultros-charts/src/data/mod.rs` (add `pub mod buckets;`)

- [ ] **Step 1: Write the failing tests**

Create `data/buckets.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::grouping::SalePoint;
    use crate::test_util::{sale, ts};

    #[test]
    fn bucket_seconds_scales_with_window() {
        assert_eq!(bucket_seconds(Some(7), 0), 6 * 3_600);
        assert_eq!(bucket_seconds(Some(30), 0), 86_400);
        assert_eq!(bucket_seconds(Some(90), 0), 86_400);
        assert_eq!(bucket_seconds(None, 2), 3_600);
        assert_eq!(bucket_seconds(None, 500), 30 * 86_400);
    }

    #[test]
    fn vwap_buckets_weight_by_quantity() {
        // 100×1 and 200×3 in the same day bucket → VWAP 175, vertex at midday
        let points = vec![
            SalePoint { ts: ts(0), price: 100, quantity: 1 },
            SalePoint { ts: ts(3_600), price: 200, quantity: 3 },
        ];
        let buckets = vwap_buckets(&points, 86_400);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].vwap, 175.0);
        assert_eq!(buckets[0].ts, ts(43_200));
    }

    #[test]
    fn volume_buckets_sum_quantities() {
        let sales = vec![
            sale(100, 2, 1, ts(0)),
            sale(100, 3, 1, ts(60)),
            sale(100, 5, 1, ts(86_400)),
        ];
        let buckets = volume_buckets(&sales, 86_400);
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].quantity, 5);
        assert_eq!(buckets[1].quantity, 5);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-charts buckets`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation**

Prepend to `data/buckets.rs`:

```rust
//! Time-bucketed aggregation: VWAP line vertices and volume bars. Bucket
//! boundaries align to absolute UTC timestamps so day/week buckets land on
//! calendar boundaries — ported from the web UI's quantity histogram.

use std::collections::BTreeMap;

use chrono::NaiveDateTime;
use ultros_api_types::SaleHistory;

use crate::data::grouping::SalePoint;

const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;

/// Bucket width for VWAP lines / volume bars. `days_range` is the user's
/// selected window (7/30/90); `None` or 0 falls back to the data span.
pub fn bucket_seconds(days_range: Option<i32>, data_span_days: i64) -> i64 {
    let effective_days = match days_range {
        Some(days) if days > 0 => days as i64,
        _ => data_span_days.max(1),
    };
    match effective_days {
        ..=2 => HOUR,
        3..=10 => 6 * HOUR,
        11..=120 => DAY,
        121..=400 => 7 * DAY,
        _ => 30 * DAY,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VwapPoint {
    /// Bucket midpoint (the line vertex sits in the middle of its bucket).
    pub ts: NaiveDateTime,
    pub vwap: f64,
}

/// Volume-weighted average price per time bucket.
pub fn vwap_buckets(points: &[SalePoint], bucket_secs: i64) -> Vec<VwapPoint> {
    if bucket_secs <= 0 {
        return Vec::new();
    }
    let mut sums: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
    for point in points {
        let bucket = point.ts.and_utc().timestamp().div_euclid(bucket_secs) * bucket_secs;
        let entry = sums.entry(bucket).or_default();
        entry.0 += point.price as i64 * point.quantity as i64;
        entry.1 += point.quantity as i64;
    }
    sums.into_iter()
        .filter(|(_, (_, quantity))| *quantity > 0)
        .filter_map(|(bucket, (gil, quantity))| {
            chrono::DateTime::from_timestamp(bucket + bucket_secs / 2, 0).map(|ts| VwapPoint {
                ts: ts.naive_utc(),
                vwap: gil as f64 / quantity as f64,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeBucket {
    /// Bucket start.
    pub ts: NaiveDateTime,
    pub quantity: i64,
}

/// Total quantity sold per bucket across all series.
pub fn volume_buckets(sales: &[SaleHistory], bucket_secs: i64) -> Vec<VolumeBucket> {
    if bucket_secs <= 0 || sales.is_empty() {
        return Vec::new();
    }
    let mut sums: BTreeMap<i64, i64> = BTreeMap::new();
    for sale in sales {
        let bucket = sale.sold_date.and_utc().timestamp().div_euclid(bucket_secs) * bucket_secs;
        *sums.entry(bucket).or_default() += sale.quantity as i64;
    }
    sums.into_iter()
        .filter_map(|(bucket, quantity)| {
            chrono::DateTime::from_timestamp(bucket, 0).map(|ts| VolumeBucket {
                ts: ts.naive_utc(),
                quantity,
            })
        })
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-charts buckets`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-charts/src/data
git commit -m "feat(charts): VWAP and volume time-bucketing" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 7: Trendline + stats

**Files:**
- Create: `ultros-frontend/ultros-charts/src/data/trend.rs`
- Create: `ultros-frontend/ultros-charts/src/data/stats.rs`
- Modify: `ultros-frontend/ultros-charts/src/data/mod.rs` (add `pub mod stats;` and `pub mod trend;`)

- [ ] **Step 1: Write the failing tests**

Create `data/trend.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_a_perfect_line() {
        let points: Vec<_> = (0..10).map(|i| (i as f64, 3.0 + 2.0 * i as f64)).collect();
        let (slope, intercept) = least_squares(&points).unwrap();
        assert!((slope - 2.0).abs() < 1e-9);
        assert!((intercept - 3.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_degenerate_input() {
        assert!(least_squares(&[]).is_none());
        assert!(least_squares(&[(1.0, 1.0)]).is_none());
        assert!(least_squares(&[(1.0, 1.0), (1.0, 2.0)]).is_none());
    }
}
```

Create `data/stats.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vwap_weights_by_quantity() {
        assert_eq!(vwap(&[(100, 1), (200, 3)]), Some(175));
        assert_eq!(vwap(&[]), None);
        assert_eq!(vwap(&[(100, 0)]), None);
    }

    #[test]
    fn median_handles_even_and_odd() {
        assert_eq!(median(&[3, 1, 2]), Some(2));
        assert_eq!(median(&[4, 1, 2, 3]), Some(2));
        assert_eq!(median(&[]), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-charts trend stats`
(Note: run as two commands if the filter doesn't take two patterns: `cargo test -p ultros-charts trend` and `cargo test -p ultros-charts stats`.)
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementations**

Prepend to `data/trend.rs`:

```rust
/// Least-squares fit over `(x, y)` points. Returns `(slope, intercept)`,
/// or `None` with fewer than 2 points or zero x-variance.
pub fn least_squares(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    if points.len() < 2 {
        return None;
    }
    let n = points.len() as f64;
    let mean_x = points.iter().map(|(x, _)| x).sum::<f64>() / n;
    let mean_y = points.iter().map(|(_, y)| y).sum::<f64>() / n;
    let mut covariance = 0.0;
    let mut variance_x = 0.0;
    for (x, y) in points {
        let dx = x - mean_x;
        covariance += dx * (y - mean_y);
        variance_x += dx * dx;
    }
    if variance_x == 0.0 {
        return None;
    }
    let slope = covariance / variance_x;
    Some((slope, mean_y - slope * mean_x))
}
```

Prepend to `data/stats.rs`:

```rust
/// Volume-weighted average price; `None` on empty input or zero total quantity.
pub fn vwap(prices_and_quantities: &[(i32, i32)]) -> Option<i32> {
    let (num, den) = prices_and_quantities
        .iter()
        .fold((0i64, 0i64), |(n, d), (price, quantity)| {
            (n + (*price as i64) * (*quantity as i64), d + (*quantity as i64))
        });
    if den == 0 {
        return None;
    }
    Some((num / den) as i32)
}

/// Median price; integer mean of the middle two for even counts.
pub fn median(prices: &[i32]) -> Option<i32> {
    if prices.is_empty() {
        return None;
    }
    let mut sorted: Vec<i32> = prices.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    if n % 2 == 1 {
        Some(sorted[n / 2])
    } else {
        Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-charts` (whole crate)
Expected: PASS (all tests so far)

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-charts/src/data
git commit -m "feat(charts): trendline fit and vwap/median stats" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 8: Price-history chart layout

**Files:**
- Create: `ultros-frontend/ultros-charts/src/charts/mod.rs`
- Create: `ultros-frontend/ultros-charts/src/charts/price_history.rs`
- Modify: `ultros-frontend/ultros-charts/src/lib.rs` (add `pub mod charts;`)

- [ ] **Step 1: Write the failing tests**

Create `charts/mod.rs`:

```rust
pub mod price_history;
```

Create `charts/price_history.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Node;
    use crate::test_util::{sale, ts, world_helper};
    use ultros_api_types::SaleHistory;

    fn count(scene: &crate::scene::Scene, predicate: impl Fn(&Node) -> bool) -> usize {
        scene.nodes.iter().filter(|n| predicate(n)).count()
    }

    fn two_world_sales() -> Vec<SaleHistory> {
        // 20 sales over ~10 days alternating between two worlds of one DC
        (0..20)
            .map(|i| sale(1_000 + i * 10, 2, 1 + (i % 2), ts(1_700_000_000 + i as i64 * 43_200)))
            .collect()
    }

    #[test]
    fn renders_lines_dots_volume_and_labels() {
        let scene = build_price_history_scene(
            &world_helper(),
            &two_world_sales(),
            &PriceChartOptions {
                title: Some("Test Item".to_string()),
                ..Default::default()
            },
        );
        let polylines = count(&scene, |n| matches!(n, Node::Polyline { .. }));
        assert_eq!(polylines, 2, "one VWAP line per world series");
        let circles = count(&scene, |n| matches!(n, Node::Circle { .. }));
        assert!(circles >= 20, "raw sale dots (plus legend chips)");
        let bars = count(&scene, |n| matches!(n, Node::Rect { .. }));
        assert!(bars >= 1, "volume lane bars");
        let texts: Vec<_> = scene
            .nodes
            .iter()
            .filter_map(|n| match n {
                Node::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"Test Item"));
        assert!(texts.contains(&"Gilgamesh"), "legend entries present");
    }

    #[test]
    fn single_series_gets_area_fill() {
        let sales: Vec<_> = (0..20)
            .map(|i| sale(1_000 + i * 10, 2, 1, ts(1_700_000_000 + i as i64 * 43_200)))
            .collect();
        let scene =
            build_price_history_scene(&world_helper(), &sales, &PriceChartOptions::default());
        assert_eq!(count(&scene, |n| matches!(n, Node::Area { .. })), 1);
        assert_eq!(count(&scene, |n| matches!(n, Node::Polyline { .. })), 1);
    }

    #[test]
    fn empty_sales_renders_no_data_card() {
        let scene = build_price_history_scene(&world_helper(), &[], &PriceChartOptions::default());
        let has_no_data_text = scene.nodes.iter().any(|n| {
            matches!(n, Node::Text { content, .. } if content == "No recent sales")
        });
        assert!(has_no_data_text);
    }
}
```

Add `pub mod charts;` to `lib.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-charts price_history`
Expected: FAIL to compile.

- [ ] **Step 3: Write the implementation**

Prepend to `charts/price_history.rs`:

```rust
//! Layout for the sale-history market chart: a VWAP line per series with
//! the raw sales as dimmed dots behind it, and a volume lane along the
//! bottom. One function builds the whole picture so the server PNG and the
//! web chart (PR 2) can never drift apart.

use std::borrow::Cow;

use chrono::TimeDelta;
use itertools::Itertools;
use ultros_api_types::world_helper::WorldHelper;
use ultros_api_types::SaleHistory;

use crate::data::buckets::{bucket_seconds, volume_buckets, vwap_buckets};
use crate::data::grouping::group_sales_by_scope;
use crate::data::outliers::filter_outliers;
use crate::data::stats::vwap;
use crate::data::trend::least_squares;
use crate::scale::{short_number, LinearScale, TimeScale};
use crate::scene::{Node, Scene, Stroke, TextAnchor};
use crate::theme::Theme;

#[derive(Clone, Debug)]
pub struct PriceChartOptions {
    pub width: f32,
    pub height: f32,
    pub remove_outliers: bool,
    pub show_market_average: bool,
    pub show_trendline: bool,
    pub show_volume: bool,
    /// Drawn in the title row, so only meaningful when `title` is set
    /// (the web chart renders its legend as HTML chips instead).
    pub show_legend: bool,
    /// Card title (item name); `None` hides the title row (web — the page
    /// already shows the item name).
    pub title: Option<String>,
    /// `data:image/png;base64,…` icon shown beside the title.
    pub icon_data_uri: Option<String>,
    /// User-selected day window (7/30/90); `None`/0 = derive from data span.
    pub days_range: Option<i32>,
    pub theme: Theme,
}

impl Default for PriceChartOptions {
    fn default() -> Self {
        Self {
            width: 960.0,
            height: 540.0,
            remove_outliers: false,
            show_market_average: true,
            show_trendline: false,
            show_volume: true,
            show_legend: true,
            title: None,
            icon_data_uri: None,
            days_range: None,
            theme: Theme::dark_card(),
        }
    }
}

pub fn build_price_history_scene(
    world_helper: &WorldHelper,
    sales: &[SaleHistory],
    options: &PriceChartOptions,
) -> Scene {
    let theme = &options.theme;
    let mut scene = Scene {
        width: options.width,
        height: options.height,
        background: theme.background,
        font_family: theme.font_family.clone(),
        nodes: Vec::new(),
    };

    let sales = if options.remove_outliers {
        filter_outliers(sales)
    } else {
        Cow::Borrowed(sales)
    };
    let series = group_sales_by_scope(world_helper, &sales);
    let all_points = || series.iter().flat_map(|s| s.points.iter());

    let Some((first_ts, last_ts)) = all_points().map(|p| p.ts).minmax().into_option() else {
        // Deliberate "no data" card instead of an error — the Discord bot
        // and the card endpoint still get a presentable image.
        scene.nodes.push(Node::Text {
            x: options.width / 2.0,
            y: options.height / 2.0,
            content: "No recent sales".to_string(),
            size: 22.0,
            color: theme.text_muted,
            anchor: TextAnchor::Middle,
            bold: false,
        });
        return scene;
    };
    let (min_price, max_price) = all_points()
        .map(|p| p.price)
        .minmax()
        .into_option()
        .expect("non-empty by the timestamp check above");

    // ── Geometry ────────────────────────────────────────────────────────
    let title_height = if options.title.is_some() { 56.0 } else { 12.0 };
    let margin_left = 68.0;
    let margin_right = 16.0;
    let margin_bottom = 32.0;
    let plot_left = margin_left;
    let plot_right = options.width - margin_right;
    let plot_top = title_height;
    let plot_bottom = options.height - margin_bottom;
    let plot_height = plot_bottom - plot_top;
    let (volume_top, price_bottom) = if options.show_volume {
        let volume_height = plot_height * 0.22;
        (plot_bottom - volume_height, plot_bottom - volume_height - 10.0)
    } else {
        (plot_bottom, plot_bottom)
    };

    let time = TimeScale::new(first_ts, last_ts, (plot_left, plot_right));
    // Don't anchor the price axis at zero: gil prices cluster far above it
    // and the signal is the variation. 5% headroom on both sides.
    let price_pad = ((max_price - min_price) as f64 * 0.05).max(1.0);
    let price = LinearScale::new(
        (
            (min_price as f64 - price_pad).max(0.0),
            max_price as f64 + price_pad,
        ),
        (price_bottom, plot_top),
    );

    // ── Grid + axis labels ──────────────────────────────────────────────
    for tick in price.ticks(5) {
        let y = price.scale(tick);
        scene.nodes.push(Node::Line {
            x1: plot_left,
            y1: y,
            x2: plot_right,
            y2: y,
            stroke: Stroke {
                color: theme.grid,
                width: 1.0,
                dash: None,
            },
        });
        scene.nodes.push(Node::Text {
            x: plot_left - 8.0,
            y: y + 4.0,
            content: short_number(tick.round() as i32),
            size: 13.0,
            color: theme.text_muted,
            anchor: TextAnchor::End,
            bold: false,
        });
    }
    for tick in time.ticks(6) {
        let x = time.scale(tick.ts);
        scene.nodes.push(Node::Text {
            x,
            y: plot_bottom + 20.0,
            content: tick.label,
            size: 13.0,
            color: theme.text_muted,
            anchor: TextAnchor::Middle,
            bold: false,
        });
    }

    // ── Volume lane ─────────────────────────────────────────────────────
    let span_days = (last_ts - first_ts).num_days();
    let bucket_secs = bucket_seconds(options.days_range, span_days);
    if options.show_volume {
        let volumes = volume_buckets(&sales, bucket_secs);
        if let Some(max_volume) = volumes.iter().map(|v| v.quantity).max() {
            let volume = LinearScale::new((0.0, max_volume as f64), (plot_bottom, volume_top));
            let bucket_px = time.scale(first_ts + TimeDelta::seconds(bucket_secs))
                - time.scale(first_ts);
            let bar_width = (bucket_px * 0.8).max(1.0);
            for bucket in &volumes {
                let center = bucket.ts + TimeDelta::seconds(bucket_secs / 2);
                let x = time.scale(center);
                let top = volume.scale(bucket.quantity as f64);
                scene.nodes.push(Node::Rect {
                    x: x - bar_width / 2.0,
                    y: top,
                    width: bar_width,
                    height: (plot_bottom - top).max(1.0),
                    rx: 1.0,
                    fill: theme.volume.with_alpha(0.7),
                });
            }
        }
    }

    // ── Raw sale dots (under the lines) ─────────────────────────────────
    let series_color = |index: usize| theme.palette[index % theme.palette.len()];
    for (index, group) in series.iter().enumerate() {
        let color = series_color(index);
        for point in &group.points {
            scene.nodes.push(Node::Circle {
                cx: time.scale(point.ts),
                cy: price.scale(point.price as f64),
                r: 2.0,
                fill: color.with_alpha(0.35),
            });
        }
    }

    // ── VWAP lines (the primary visual) ─────────────────────────────────
    for (index, group) in series.iter().enumerate() {
        let color = series_color(index);
        let line: Vec<(f32, f32)> = vwap_buckets(&group.points, bucket_secs)
            .into_iter()
            .map(|p| (time.scale(p.ts), price.scale(p.vwap)))
            .collect();
        if line.len() > 1 {
            if series.len() == 1 {
                scene.nodes.push(Node::Area {
                    points: line.clone(),
                    baseline_y: price_bottom,
                    fill: color.with_alpha(0.08),
                });
            }
            scene.nodes.push(Node::Polyline {
                points: line,
                stroke: Stroke {
                    color,
                    width: 2.0,
                    dash: None,
                },
            });
        }
    }

    // ── Overlays ────────────────────────────────────────────────────────
    if options.show_market_average {
        let pairs: Vec<(i32, i32)> = all_points().map(|p| (p.price, p.quantity)).collect();
        if let Some(market_average) = vwap(&pairs) {
            let y = price.scale(market_average as f64);
            scene.nodes.push(Node::Line {
                x1: plot_left,
                y1: y,
                x2: plot_right,
                y2: y,
                stroke: Stroke {
                    color: theme.market_average.with_alpha(0.9),
                    width: 1.5,
                    dash: Some((2.0, 4.0)),
                },
            });
        }
    }
    if options.show_trendline {
        let points: Vec<(f64, f64)> = all_points()
            .map(|p| (p.ts.and_utc().timestamp() as f64, p.price as f64))
            .collect();
        if let Some((slope, intercept)) = least_squares(&points) {
            let x1 = first_ts.and_utc().timestamp() as f64;
            let x2 = last_ts.and_utc().timestamp() as f64;
            scene.nodes.push(Node::Line {
                x1: time.scale(first_ts),
                y1: price.scale(intercept + slope * x1),
                x2: time.scale(last_ts),
                y2: price.scale(intercept + slope * x2),
                stroke: Stroke {
                    color: theme.trend.with_alpha(0.8),
                    width: 1.5,
                    dash: Some((6.0, 4.0)),
                },
            });
        }
    }

    // ── Title row: icon + title left, legend chips right ───────────────
    if let Some(title) = &options.title {
        let mut x = 16.0;
        if let Some(icon) = &options.icon_data_uri {
            scene.nodes.push(Node::Image {
                x,
                y: 8.0,
                width: 40.0,
                height: 40.0,
                href: icon.clone(),
            });
            x += 48.0;
        }
        scene.nodes.push(Node::Text {
            x,
            y: 36.0,
            content: title.clone(),
            size: 24.0,
            color: theme.text,
            anchor: TextAnchor::Start,
            bold: true,
        });
    }
    if options.show_legend && options.title.is_some() && series.len() > 1 {
        // Right-aligned row of "● Name" chips. 7px per char approximates
        // Jaldi at 13px — close enough for a legend.
        let mut x = plot_right;
        for (index, group) in series.iter().enumerate().rev() {
            x -= group.name.len() as f32 * 7.0;
            scene.nodes.push(Node::Text {
                x,
                y: 32.0,
                content: group.name.clone(),
                size: 13.0,
                color: theme.text,
                anchor: TextAnchor::Start,
                bold: false,
            });
            x -= 12.0;
            scene.nodes.push(Node::Circle {
                cx: x + 4.0,
                cy: 28.0,
                r: 4.0,
                fill: series_color(index),
            });
            x -= 14.0;
        }
    }

    scene
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-charts price_history`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-charts/src/charts ultros-frontend/ultros-charts/src/lib.rs
git commit -m "feat(charts): price-history scene layout (VWAP lines, dots, volume lane)" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 9: Icon embedding (image feature)

**Files:**
- Create: `ultros-frontend/ultros-charts/src/icon.rs`
- Modify: `ultros-frontend/ultros-charts/Cargo.toml` (add base64 to deps + feature)
- Modify: `ultros-frontend/ultros-charts/src/lib.rs`

- [ ] **Step 1: Add the base64 dependency**

In `ultros-frontend/ultros-charts/Cargo.toml`, add to `[dependencies]`:

```toml
base64 = { version = "0.22", optional = true }
```

and change the feature line to:

```toml
[features]
image = ["dep:ultros-xiv-icons", "dep:image", "dep:base64"]
```

- [ ] **Step 2: Write the failing test**

Create `icon.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_webp_to_png_data_uri() {
        let img = image::DynamicImage::new_rgb8(4, 4);
        let mut webp = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut webp), image::ImageFormat::WebP)
            .unwrap();
        let uri = encode_png_data_uri(&webp).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
    }
}
```

Add to `lib.rs`:

```rust
#[cfg(feature = "image")]
mod icon;
#[cfg(feature = "image")]
pub use icon::item_icon_data_uri;
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p ultros-charts --features image icon`
Expected: FAIL to compile.

- [ ] **Step 4: Write the implementation**

Prepend to `icon.rs`:

```rust
//! Item icons for SVG embedding. The source assets are WebP, which resvg
//! can't decode — transcode to PNG and inline as a data URI.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ultros_api_types::icon_size::IconSize;

/// Item icon as a `data:image/png;base64,…` URI, or `None` if there is no
/// icon for this item.
pub fn item_icon_data_uri(item_id: i32) -> Option<String> {
    let webp = ultros_xiv_icons::get_item_image(item_id, IconSize::Medium)?;
    encode_png_data_uri(webp)
}

fn encode_png_data_uri(webp: &[u8]) -> Option<String> {
    let decoded = image::load_from_memory_with_format(webp, image::ImageFormat::WebP).ok()?;
    let mut png = Vec::new();
    decoded
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(format!("data:image/png;base64,{}", STANDARD.encode(&png)))
}
```

(If `get_item_image` returns something other than `Option<&[u8]>`, check its signature in `ultros-frontend/ultros-xiv-icons/src/lib.rs` and adapt the call — the old plotters code called it exactly like this, so it should match.)

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p ultros-charts --features image icon`
Expected: PASS. Also run `cargo test -p ultros-charts` (no features) to confirm the crate still builds without the feature.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-charts/src/icon.rs ultros-frontend/ultros-charts/src/lib.rs ultros-frontend/ultros-charts/Cargo.toml Cargo.lock
git commit -m "feat(charts): item icon as embedded PNG data URI" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 10: Design-eyeball example

**Files:**
- Create: `ultros-frontend/ultros-charts/examples/price_history.rs`

- [ ] **Step 1: Write the example**

```rust
//! Renders a sample chart to sample-chart.svg for design eyeballing.
//! Run: cargo run -p ultros-charts --example price_history

use chrono::DateTime;
use ultros_api_types::world::{Datacenter, Region, World, WorldData};
use ultros_api_types::world_helper::WorldHelper;
use ultros_api_types::SaleHistory;
use ultros_charts::charts::price_history::{build_price_history_scene, PriceChartOptions};
use ultros_charts::svg::scene_to_svg;

fn lcg(state: &mut u32) -> i32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*state >> 16) as i32
}

fn main() {
    let helper = WorldHelper::new(WorldData {
        regions: vec![Region {
            id: 1,
            name: "North-America".to_string(),
            datacenters: vec![Datacenter {
                id: 1,
                name: "Aether".to_string(),
                region_id: 1,
                worlds: vec![
                    World { id: 1, name: "Gilgamesh".to_string(), datacenter_id: 1 },
                    World { id: 2, name: "Adamantoise".to_string(), datacenter_id: 1 },
                ],
            }],
        }],
    });
    let mut state = 0x1234_5678u32;
    let sales: Vec<SaleHistory> = (0..200)
        .map(|i| SaleHistory {
            id: i,
            quantity: 1 + (lcg(&mut state) % 5).abs(),
            price_per_item: 8_000 + lcg(&mut state) % 400 + if i > 120 { 1_500 } else { 0 },
            buying_character_id: 0,
            hq: false,
            sold_item_id: 1,
            sold_date: DateTime::from_timestamp(1_750_000_000 + i as i64 * 7_200, 0)
                .unwrap()
                .naive_utc(),
            world_id: 1 + (i % 2),
            buyer_name: None,
        })
        .collect();
    let scene = build_price_history_scene(
        &helper,
        &sales,
        &PriceChartOptions {
            title: Some("Grade 8 Tincture of Intelligence - Sale History".to_string()),
            show_trendline: true,
            remove_outliers: true,
            ..Default::default()
        },
    );
    std::fs::write("sample-chart.svg", scene_to_svg(&scene)).unwrap();
    println!("wrote sample-chart.svg");
}
```

- [ ] **Step 2: Run it and eyeball the output**

Run: `cargo run -p ultros-charts --example price_history`
Expected: prints `wrote sample-chart.svg`.

Open `sample-chart.svg` in a browser (agents: use the chrome-devtools MCP to screenshot `file:///C:/Users/chw11/code/ultros/sample-chart.svg`). Check: dark background, two colored VWAP lines with dimmed dots behind them, green volume bars in a bottom lane, horizontal-only grid, legend chips top-right, readable axis labels. Fix layout constants if anything overlaps. **This is the design checkpoint — show the result to the user / report what it looks like in the task summary.**

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-charts/examples/price_history.rs
git commit -m "docs(charts): sample-render example for design review" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 11: Swap item_card.rs, delete plotters

**Files:**
- Modify: `ultros/src/web/item_card.rs` (full rewrite, shown below)
- Modify: `ultros-frontend/ultros-charts/src/lib.rs` (delete all old plotters code)
- Modify: `ultros-frontend/ultros-charts/Cargo.toml` (drop plotters, xiv-gen, xiv-gen-db, anyhow)
- Modify: `ultros/Cargo.toml` (drop plotters-svg)
- Modify: `Cargo.toml` (workspace — drop `plotters = "0.3.7"`; possibly the pathfinder_simd patch)

- [ ] **Step 1: Rewrite item_card.rs**

Replace the entire contents of `ultros/src/web/item_card.rs` with:

```rust
use std::sync::Arc;

use super::{WebState, error::WebError};
use anyhow::{Result, anyhow};
use axum::{
    body::Body,
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use hyper::header;
use resvg::{
    tiny_skia,
    usvg::{self, Options},
};
use ultros_api_types::{
    SaleHistory,
    world_helper::{AnyResult, WorldHelper},
};
use ultros_charts::charts::price_history::{PriceChartOptions, build_price_history_scene};
use ultros_charts::svg::scene_to_svg;
use ultros_db::UltrosDb;
use xiv_gen::{Item, ItemId};

pub(crate) async fn generate_image(
    db: &UltrosDb,
    world_helper: &WorldHelper,
    item: &'static Item,
    world: &AnyResult<'_>,
) -> Result<Vec<u8>> {
    let world_ids: Vec<_> = world.all_worlds().map(|w| w.id).collect();
    let sales: Vec<SaleHistory> = db
        .get_sale_history_from_multiple_worlds(world_ids.into_iter(), item.key_id.0, 200)
        .await?
        .into_iter()
        .map(SaleHistory::from)
        .collect();
    let scene = build_price_history_scene(
        world_helper,
        &sales,
        &PriceChartOptions {
            remove_outliers: true,
            title: Some(format!("{} - Sale History", item.name)),
            icon_data_uri: ultros_charts::item_icon_data_uri(item.key_id.0),
            ..Default::default()
        },
    );
    svg_to_png(&scene_to_svg(&scene))
}

fn svg_to_png(svg: &str) -> Result<Vec<u8>> {
    let mut opt = Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(svg, &opt)?;
    let pixmap_size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height())
        .ok_or(anyhow!("failed to make pixmap"))?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    Ok(pixmap.encode_png()?)
}

#[axum_macros::debug_handler(state = WebState)]
pub(crate) async fn item_card(
    Path((world, item_id)): Path<(String, i32)>,
    State(db): State<UltrosDb>,
    State(world_helper): State<Arc<WorldHelper>>,
) -> Result<impl IntoResponse, WebError> {
    let item = xiv_gen_db::data()
        .items
        .get(&ItemId(item_id))
        .ok_or(WebError::InvalidItemId(item_id))?;
    let world = world_helper
        .lookup_world_by_name(&world)
        .ok_or_else(|| WebError::WorldNotFound(world))?;
    let bytes = generate_image(&db, &world_helper, item, &world).await?;
    let mime_type = mime_guess::from_path("icon.png").first_or_text_plain();
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, mime_type.as_ref())
        .body(Body::new(http_body_util::Full::from(bytes)))?)
}

#[cfg(test)]
mod tests {
    use super::svg_to_png;
    use chrono::DateTime;
    use ultros_api_types::SaleHistory;
    use ultros_api_types::world::{Datacenter, Region, World, WorldData};
    use ultros_api_types::world_helper::WorldHelper;
    use ultros_charts::charts::price_history::{PriceChartOptions, build_price_history_scene};
    use ultros_charts::svg::scene_to_svg;

    #[test]
    fn renders_a_decodable_png() {
        let helper = WorldHelper::new(WorldData {
            regions: vec![Region {
                id: 1,
                name: "Test".to_string(),
                datacenters: vec![Datacenter {
                    id: 1,
                    name: "DC".to_string(),
                    region_id: 1,
                    worlds: vec![World {
                        id: 1,
                        name: "World".to_string(),
                        datacenter_id: 1,
                    }],
                }],
            }],
        });
        let sales: Vec<SaleHistory> = (0..30)
            .map(|i| SaleHistory {
                id: i,
                quantity: 1,
                price_per_item: 1_000 + i * 13,
                buying_character_id: 0,
                hq: false,
                sold_item_id: 1,
                sold_date: DateTime::from_timestamp(1_750_000_000 + i as i64 * 7_200, 0)
                    .unwrap()
                    .naive_utc(),
                world_id: 1,
                buyer_name: None,
            })
            .collect();
        let scene = build_price_history_scene(
            &helper,
            &sales,
            &PriceChartOptions {
                title: Some("Smoke Test - Sale History".to_string()),
                remove_outliers: true,
                ..Default::default()
            },
        );
        let png = svg_to_png(&scene_to_svg(&scene)).expect("png");
        let decoded = image::load_from_memory(&png).expect("decodable png");
        assert_eq!((decoded.width(), decoded.height()), (960, 540));
    }
}
```

Notes vs. the old version: the bogus `resources_dir: canonicalize(<svg content>)` (always `None` — it canonicalized the SVG string as if it were a path) is dropped; `Rc<RefCell<SVGBackend>>` dance is gone; the theme defaults to `dark_card()` so no explicit background tuple.

- [ ] **Step 2: Delete the old plotters code from lib.rs**

Replace the entire contents of `ultros-frontend/ultros-charts/src/lib.rs` with:

```rust
//! ultros_charts — chart rendering for FFXIV market data.
//!
//! Pure-Rust scene-graph core: chart layouts in [`charts`] build a
//! renderer-agnostic [`scene::Scene`], which [`svg::scene_to_svg`] turns
//! into an SVG string (rasterized to PNG by the server via resvg). PR 2
//! adds a Leptos renderer over the same scenes; PR 3 sparklines.

pub mod charts;
pub mod data;
pub mod scale;
pub mod scene;
pub mod svg;
pub mod theme;

#[cfg(feature = "image")]
mod icon;
#[cfg(feature = "image")]
pub use icon::item_icon_data_uri;

#[cfg(test)]
pub(crate) mod test_util;
```

- [ ] **Step 3: Purge the dependencies**

Replace the `[dependencies]` section of `ultros-frontend/ultros-charts/Cargo.toml` with:

```toml
[dependencies]
ultros-api-types = { path = "../../ultros-api-types" }
chrono = { workspace = true }
itertools.workspace = true
ultros-xiv-icons = { path = "../ultros-xiv-icons", optional = true }
image = { workspace = true, optional = true }
base64 = { version = "0.22", optional = true }
```

(removes `plotters`, `xiv-gen-db`, `xiv-gen`, `anyhow`)

In `ultros/Cargo.toml`, delete the line:

```toml
plotters-svg = { version = "0.3.7", features = ["bitmap_encoder"] }
```

In the workspace `Cargo.toml`, delete the line:

```toml
plotters = "0.3.7"
```

- [ ] **Step 4: Check whether the pathfinder_simd patch is now dead**

Run: `cargo tree -i pathfinder_simd`
- If it prints "package ID specification ... did not match any packages" (nothing depends on it anymore — it came in via plotters' font stack), also delete from the workspace `Cargo.toml`:

```toml
pathfinder_simd = { git = "https://github.com/servo/pathfinder.git" } # needed on ARM Mac until pathfinder_simd has new release
```

- If something still depends on it, leave the patch alone and note what in the PR description.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test -p ultros-charts --features image && cargo test -p ultros --lib web::item_card`
Expected: PASS, including the new `renders_a_decodable_png` smoke test. (The `ultros` test build is slow the first time — vendored OpenSSL — but cached afterwards.)

- [ ] **Step 6: Verify plotters is fully gone**

Run: `grep -ri plotters --include="*.toml" --include="*.rs" .` (or `rg -i plotters -g '*.toml' -g '*.rs'`)
Expected: no hits outside Cargo.lock/docs. Then run `cargo update --workspace` is NOT needed — just confirm `Cargo.lock` no longer lists plotters after the next build (`cargo check -p ultros` will rewrite it).

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: replace plotters with ultros_charts scene renderer for item card PNGs" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 12: CI gate + PR

- [ ] **Step 1: Run check_ci**

From the repo root (Git Bash): `./check_ci.sh`
Expected: fmt-check and clippy both clean. Fix anything reported (`cargo fmt --all` for formatting; read and fix clippy warnings — no blanket `#[allow]`).

- [ ] **Step 2: Push and open the PR**

```bash
git push -u origin ultros-charts-rewrite
gh pr create --title "feat: ultros_charts scene-graph core + PNG path (replaces plotters)" --body "PR 1 of 3 for the ultros_charts rewrite (spec: docs/superpowers/specs/2026-06-09-ultros-charts-design.md).

- New scene-graph core in ultros-charts: data transforms (IQR/grouping/VWAP buckets/trendline), scales, display list, SVG serializer
- Discord/item-card PNG endpoint now renders the redesigned chart (VWAP lines + dimmed sale dots + volume lane) via the same resvg pipeline
- plotters and plotters-svg removed from the workspace
- Sample render: cargo run -p ultros-charts --example price_history

PR 2 will move the web price chart onto this (removing leptos-chartistry); PR 3 the sparklines.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

- [ ] **Step 3: Verify the no-data path renders (quick sanity)**

Run: `cargo test -p ultros-charts price_history::tests::empty_sales_renders_no_data_card`
Expected: PASS (already covered; this is the final sanity check that empty market data can't 500 the card endpoint).

---

## Out of scope for this PR (later plans)

- **PR 2:** `leptos` feature — `<PriceHistoryChart>` component rendering scenes as reactive SVG, hover/crosshair/tooltip, legend toggle chips, stats strip; remove leptos-chartistry; rewrite `price_history_chart.rs` as wiring; i18n keys for any new visible strings in all 7 locales.
- **PR 3:** `<Sparkline>` interactive component + gap-interpolation port; migrate Market Movers, Continue Tracking, Trends, Analyzer; delete `sparkline.rs`.
