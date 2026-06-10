# ultros_charts PR 3 — Interactive Sparklines Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move sparkline math into ultros_charts and add hover interactivity (dot + micro-tooltip) to the app's `<Sparkline>`, with zero call-site changes.

**Architecture:** Core gains `data/interpolate.rs` (the gap-interpolation, moved verbatim with its tests) and `charts/sparkline.rs` (`build_sparkline` → `SparklineModel`: scaled vertices, interpolated values, trend color, nearest-index hover math). The app's `sparkline.rs` becomes a thin interactive renderer over the model: same polyline by construction, plus a pointer-driven dot and a tooltip showing the value and "Nh ago". Two NEW i18n keys (all 7 locales). Per user instruction this lands on the existing `ultros-charts-web` branch as part of one combined PR.

**Note:** This was originally scoped as PR 3 of the spec (docs/superpowers/specs/2026-06-09-ultros-charts-design.md); the user requested it be folded into the web-chart PR.

---

## Context for the implementer

- Branch `ultros-charts-web` (checked out). The crate already has `scene::Color`, `components::color_attr` (leptos feature), `scale::short_number`.
- The current component is `ultros-frontend/ultros-app/src/components/sparkline.rs` (~190 lines incl. `interpolate_gaps` + 4 tests). Call sites (UNCHANGED — prop-compatible): market_movers.rs, recently_viewed.rs, trends.rs, analyzer.rs. All sparkline series are hourly VWAP.
- **i18n:** exactly two new keys, `sparkline_now` and `sparkline_hours_ago` — they MUST be added to ALL 7 locale files (`ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json`, flat structure, place near the `chart_*` keys) with the translations given below. Use the `t_string!(...).replace("{n}", ...)` pattern like `chart_stat_n_sales`.
- A `cargo leptos serve` watcher may be running — coordinate with the controller; run `cargo test -p ultros-charts` only when told the target lock is free.
- NEVER `git add -A`. Commit messages end with the Co-Authored-By trailer.

### Task 1: Core sparkline model

**Files:**
- Create: `ultros-frontend/ultros-charts/src/data/interpolate.rs`
- Create: `ultros-frontend/ultros-charts/src/charts/sparkline.rs`
- Modify: `ultros-frontend/ultros-charts/src/data/mod.rs` (+ `pub mod interpolate;`), `src/charts/mod.rs` (+ `pub mod sparkline;`)

- [ ] **Step 1:** Create `data/interpolate.rs` by MOVING `interpolate_gaps` and its 4 tests verbatim from the app's `sparkline.rs` (lines ~115–190), making the function `pub` and adapting the doc comment's first line to module context. Do not change the algorithm.

- [ ] **Step 2:** Write the failing tests in `charts/sparkline.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_min_to_bottom_and_max_to_top_inset() {
        let m = build_sparkline(&[100, 200], 1.0, 80.0, 24.0);
        assert_eq!(m.points.len(), 2);
        assert_eq!(m.points[0], (0.0, 22.0)); // min → height - inset
        assert_eq!(m.points[1], (80.0, 2.0)); // max → inset
        assert_eq!(m.values, vec![100.0, 200.0]);
    }

    #[test]
    fn all_zero_series_is_empty() {
        let m = build_sparkline(&[0, 0, 0], 0.0, 80.0, 24.0);
        assert!(m.is_empty());
        let m = build_sparkline(&[], 0.0, 80.0, 24.0);
        assert!(m.is_empty());
    }

    #[test]
    fn trend_color_follows_pct_change() {
        use crate::scene::Color;
        assert_eq!(build_sparkline(&[1, 2], 5.0, 80.0, 24.0).color, Color::hex("#34d399"));
        assert_eq!(build_sparkline(&[1, 2], -5.0, 80.0, 24.0).color, Color::hex("#f87171"));
        assert_eq!(build_sparkline(&[1, 2], 0.0, 80.0, 24.0).color, Color::hex("#94a3b8"));
    }

    #[test]
    fn nearest_index_snaps_and_clamps() {
        let m = build_sparkline(&[10, 20, 30], 0.0, 80.0, 24.0); // step = 40
        assert_eq!(m.nearest_index(-10.0), Some(0));
        assert_eq!(m.nearest_index(15.0), Some(0));
        assert_eq!(m.nearest_index(25.0), Some(1));
        assert_eq!(m.nearest_index(999.0), Some(2));
        assert_eq!(build_sparkline(&[], 0.0, 80.0, 24.0).nearest_index(10.0), None);
    }
}
```

- [ ] **Step 3:** Run `cargo test -p ultros-charts sparkline` — expect compile failure.

- [ ] **Step 4:** Implement (prepend to `charts/sparkline.rs`):

```rust
//! Geometry for the tiny inline sparklines (Market Movers, Continue
//! Tracking, Trends, Analyzer). Same visual rules as the old hand-rolled
//! component: 2px vertical inset, min→bottom / max→top scaling, gap
//! interpolation across quiet hours, single trend color.

use crate::data::interpolate::interpolate_gaps;
use crate::scene::Color;

#[derive(Clone, Debug, PartialEq)]
pub struct SparklineModel {
    pub width: f32,
    pub height: f32,
    /// Scaled polyline vertices; empty for an empty/all-zero series.
    pub points: Vec<(f32, f32)>,
    /// Interpolated values aligned with `points` (tooltip content).
    pub values: Vec<f32>,
    /// Trend color: emerald up, red down, slate flat.
    pub color: Color,
}

impl SparklineModel {
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Nearest sample index for pixel `x` (viewBox units).
    pub fn nearest_index(&self, x: f32) -> Option<usize> {
        if self.points.is_empty() {
            return None;
        }
        if self.points.len() == 1 {
            return Some(0);
        }
        let step = self.width / (self.points.len() as f32 - 1.0);
        let index = (x / step).round() as i64;
        Some(index.clamp(0, self.points.len() as i64 - 1) as usize)
    }
}

fn trend_color(pct_change: f32) -> Color {
    if pct_change > 0.0 {
        Color::hex("#34d399")
    } else if pct_change < 0.0 {
        Color::hex("#f87171")
    } else {
        Color::hex("#94a3b8")
    }
}

pub fn build_sparkline(raw: &[u32], pct_change: f32, width: f32, height: f32) -> SparklineModel {
    let filled = interpolate_gaps(raw);
    let color = trend_color(pct_change);
    if filled.iter().all(|&v| v == 0.0) {
        return SparklineModel {
            width,
            height,
            points: Vec::new(),
            values: Vec::new(),
            color,
        };
    }
    let (min, max) = filled
        .iter()
        .copied()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v), hi.max(v))
        });
    let span = (max - min).max(1.0);
    let inset = 2.0;
    let usable_h = height - inset * 2.0;
    let n = filled.len();
    let step = if n > 1 { width / (n as f32 - 1.0) } else { 0.0 };
    let points = filled
        .iter()
        .enumerate()
        .map(|(i, &v)| (i as f32 * step, inset + (1.0 - (v - min) / span) * usable_h))
        .collect();
    SparklineModel {
        width,
        height,
        points,
        values: filled,
        color,
    }
}
```

- [ ] **Step 5:** `cargo test -p ultros-charts` — all pass (interpolate tests moved + 4 new sparkline tests).

- [ ] **Step 6:** Commit:

```bash
git add ultros-frontend/ultros-charts/src/data/interpolate.rs ultros-frontend/ultros-charts/src/data/mod.rs ultros-frontend/ultros-charts/src/charts/sparkline.rs ultros-frontend/ultros-charts/src/charts/mod.rs
git commit -m "feat(charts): sparkline model with gap interpolation and hover math" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 2: Interactive app component + i18n keys

**Files:**
- Rewrite: `ultros-frontend/ultros-app/src/components/sparkline.rs`
- Modify: ALL 7 of `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json`

- [ ] **Step 1:** Add the two keys to every locale file (flat JSON, near the `chart_*` keys):

| locale | `sparkline_hours_ago` | `sparkline_now` |
|---|---|---|
| en | `"{n}h ago"` | `"now"` |
| fr | `"il y a {n} h"` | `"maintenant"` |
| de | `"vor {n} Std."` | `"jetzt"` |
| ja | `"{n}時間前"` | `"現在"` |
| cn | `"{n}小时前"` | `"现在"` |
| ko | `"{n}시간 전"` | `"지금"` |
| tc | `"{n}小時前"` | `"現在"` |

- [ ] **Step 2:** Replace `sparkline.rs` wholesale:

```rust
//! Inline SVG sparkline for the Market Movers list and other surfaces that
//! want a 24h price trace next to each row.
//!
//! Geometry/coloring live in `ultros_charts::charts::sparkline`; this
//! component adds the interactive layer: nothing renders until hover, then
//! a dot on the trace and a micro-tooltip with the value and how long ago
//! that sample was. Sparkline series are hourly VWAP, oldest first.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use ultros_charts::charts::sparkline::build_sparkline;
use ultros_charts::components::color_attr;
use ultros_charts::scale::short_number;

use crate::i18n::{t_string, use_i18n};

#[component]
pub fn Sparkline(
    /// VWAP series, oldest first. Zeros mean "no trade in this hour" and are
    /// interpolated across.
    points: Vec<u32>,
    /// Drives stroke color. Pass the API's `pct_change_24h`.
    #[prop(default = 0.0)]
    pct_change: f32,
    /// Pixel width of the rendered sparkline. Default 80.
    #[prop(default = 80)]
    width: u32,
    /// Pixel height. Default 24.
    #[prop(default = 24)]
    height: u32,
    /// Hours represented by one point step (all current feeds are hourly).
    #[prop(default = 1)]
    hours_per_point: u32,
) -> impl IntoView {
    let i18n = use_i18n();
    let model = build_sparkline(&points, pct_change, width as f32, height as f32);

    // Empty / all-zero series → render nothing rather than a flat line at
    // the bottom. The page typically shows the price as text anyway.
    if model.is_empty() {
        return view! { <span class="inline-block w-20 h-6" /> }.into_any();
    }

    let path: String = model
        .points
        .iter()
        .map(|(x, y)| format!("{x:.1},{y:.1}"))
        .collect::<Vec<_>>()
        .join(" ");
    let stroke = color_attr(&model.color);
    let model = StoredValue::new(model);
    let hover = RwSignal::new(None::<usize>);

    let on_pointer_move = move |evt: web_sys::PointerEvent| {
        let Some(target) = evt
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        let rect = target.get_bounding_client_rect();
        if rect.width() <= 0.0 {
            return;
        }
        let x_css = evt.client_x() as f64 - rect.left();
        let index =
            model.with_value(|m| m.nearest_index((x_css / rect.width()) as f32 * m.width));
        hover.set(index);
    };

    view! {
        <span
            class="relative inline-block align-middle"
            on:pointermove=on_pointer_move
            on:pointerleave=move |_| hover.set(None)
        >
            <svg
                width=width
                height=height
                viewBox=format!("0 0 {width} {height}")
                class="block"
                aria-hidden="true"
            >
                <polyline
                    fill="none"
                    stroke=stroke
                    stroke-width="1.5"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    points=path
                />
                {move || {
                    hover
                        .get()
                        .and_then(|i| {
                            model
                                .with_value(|m| {
                                    let (x, y) = *m.points.get(i)?;
                                    Some(view! {
                                        <circle
                                            cx=format!("{x:.1}")
                                            cy=format!("{y:.1}")
                                            r="2.5"
                                            fill=color_attr(&m.color)
                                        />
                                    })
                                })
                        })
                }}
            </svg>
            {move || {
                hover
                    .get()
                    .and_then(|i| {
                        model
                            .with_value(|m| {
                                let value = *m.values.get(i)?;
                                let steps_back = (m.values.len() - 1 - i) as u32 * hours_per_point;
                                let when = if steps_back == 0 {
                                    t_string!(i18n, sparkline_now).to_string()
                                } else {
                                    t_string!(i18n, sparkline_hours_ago)
                                        .to_string()
                                        .replace("{n}", &steps_back.to_string())
                                };
                                let left_pct = if m.points.len() > 1 {
                                    i as f32 / (m.points.len() as f32 - 1.0) * 100.0
                                } else {
                                    50.0
                                };
                                let style = if left_pct > 50.0 {
                                    format!("left:{left_pct:.0}%;transform:translate(-100%,-100%)")
                                } else {
                                    format!("left:{left_pct:.0}%;transform:translateY(-100%)")
                                };
                                Some(view! {
                                    <span
                                        class="pointer-events-none absolute top-0 z-20 whitespace-nowrap rounded border border-[color:var(--color-outline)] bg-violet-950/95 px-1.5 py-0.5 text-[10px] tabular-nums text-[color:var(--color-text)] shadow"
                                        style=style
                                    >
                                        {format!("{} · {}", short_number(value.round() as i32), when)}
                                    </span>
                                })
                            })
                    })
            }}
        </span>
    }
    .into_any()
}
```

(The old `interpolate_gaps` + tests are GONE from this file — they moved to core in Task 1. If `wasm_bindgen::JsCast` needs a different import path or `client_x()` typing differs, match what `price_history_chart.rs` does — it compiled with the identical pattern.)

- [ ] **Step 3:** Compile gates:
  - `cargo check -p ultros-app`
  - `cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown`
  - Confirm no call site needed changes: `rg "Sparkline" ultros-frontend/ultros-app/src --type rust -l` and check each still passes only existing props.

- [ ] **Step 4:** Commit:

```bash
git add ultros-frontend/ultros-app/src/components/sparkline.rs ultros-frontend/ultros-app/locales
git commit -m "feat: interactive sparklines (hover dot + value/time micro-tooltip)" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```
