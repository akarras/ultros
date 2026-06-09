# ultros_charts — unified chart rendering crate

**Date:** 2026-06-09
**Status:** Approved

## Goal

Rewrite `ultros-frontend/ultros-charts` as a pure-Rust charting crate focused on
FFXIV time-series market data, with one design rendered to two targets:

- **PNG** on the server (Discord bot embeds, the `/item/{world}/{item_id}` card endpoint)
- **Interactive SVG** in the browser via Leptos (item page price history, sparklines)

This removes `plotters` + `plotters-svg` from the workspace and `leptos-chartistry`
from `ultros-app`, and replaces the hand-rolled `sparkline.rs` SVG with interactive
sparklines from the same crate.

## Why

Today the same chart exists three times with three implementations that have
drifted apart visually and duplicate logic (IQR outlier filtering, world/DC/region
grouping, and trendline fitting exist twice each):

1. `ultros-charts/src/lib.rs` — plotters scatter plot → SVG → resvg → PNG (Discord/card)
2. `ultros-app/src/components/price_history_chart.rs` — leptos-chartistry, ~1050 lines (web)
3. `ultros-app/src/components/sparkline.rs` — hand-rolled SVG polyline (tables)

## Architecture

**Scene-graph core with two thin renderers.** The crate's heart is pure math plus a
renderer-agnostic display list. One layout function builds a `Scene` per chart; an
SVG-string serializer feeds the existing resvg→PNG pipeline on the server, and a
Leptos module renders the same `Scene` as reactive SVG nodes in the browser. The
Discord PNG and the web chart are the same design by construction.

Alternatives rejected:

- *Shared math only, two bespoke renderers* — re-creates today's drift problem.
- *Adopt another off-the-shelf crate* (poloto, charming, …) — none do interactive
  WASM + server PNG from one codebase in pure Rust without heavy deps.

### Crate layout (`ultros-frontend/ultros-charts`, lib name `ultros_charts`)

| Module | Responsibility |
|---|---|
| `data/` | IQR outlier filter, world/DC/region grouping, VWAP time-bucketing, least-squares trendline, sparkline gap-interpolation. Consolidates logic currently duplicated between lib.rs and price_history_chart.rs. Pure functions, unit-tested. |
| `scale.rs` | Time + linear scales, nice-tick generation (hour/6h/day/week/month steps), K/mil short-number formatting. |
| `scene.rs` | Display-list primitives (`Group`, `Path`, `Circle`, `Rect`, `Text`, `Image`) and a `Theme` (colors, fonts, background). |
| `charts/price_history.rs` | Inputs + options → `Scene` for the market chart. Single layout used by web and PNG. |
| `charts/sparkline.rs` | Sparkline geometry → `Scene`. |
| `svg.rs` | `Scene` → SVG string. Always compiled; the server rasterizes this output. |
| `leptos/` | Behind the `leptos` feature: `<PriceHistoryChart>` and `<Sparkline>` components plus the interactivity layer. |

### Features

- **default** — core + SVG serializer (no Leptos).
- **`leptos`** — the components; used by ultros-app under both `ssr` and `hydrate`.
- **`image`** — item-icon overlay, embedded as a base64 `<image>` element in the SVG
  (resvg renders embedded raster images) instead of plotters' bitmap blit. Keeps its
  current meaning of pulling in `ultros-xiv-icons` + `image`.

### Dependencies

chrono, itertools, base64, optional leptos, and the existing
xiv-gen / xiv-gen-db / ultros-api-types / ultros-xiv-icons. **No plotters.**
resvg/usvg/tiny-skia stay in the `ultros` crate where the PNG rasterization already
lives; they are pure Rust.

## Visual design

The readability pass — applies identically to web and PNG:

- **Primary visual:** bucketed VWAP line per series (world/DC/region grouping as
  today), with a soft gradient area fill when a single series is shown.
- **Raw sales:** small dimmed dots (r ≈ 2, ~35% opacity) behind the line, colored by
  series — context, not the whole chart.
- **Volume:** bars in a separate bottom lane (~22% of height) sharing the x-axis,
  bucket-aligned with the VWAP line.
- **Chrome:** horizontal-only gridlines, no axis spines, y-labels left, time labels
  bottom. Existing `CATEGORY_PALETTE` kept.
- **Overlays (toggles, as today):** least-squares trendline (dashed slate), market
  average / VWAP line (dotted yellow).
- **Carried over:** IQR outlier filter toggle, 7/30/90/All window selector, stats
  strip (n, market avg, median, min, max), world/DC/region grouping selector.
- **PNG card:** identical layout plus chart title and item icon; theme background
  `#202124` as today. Web uses the site's dark-violet theme via `Theme`.

## Interactivity (web)

- A transparent capture rect over the plot area drives a `HoverState` signal.
- On hover: vertical crosshair, a highlighted dot per series at the hovered bucket,
  and a tooltip rendered as an absolutely-positioned **HTML div** (not SVG text —
  better wrapping, Tailwind-stylable) showing time, per-series VWAP, and volume.
- Pixel-x → bucket hover resolution is core math (binary search), unit-testable.
- Legend chips toggle series visibility.
- **Sparklines:** pointer-over shows a dot + micro-tooltip ("12,345 gil · 14h ago",
  derived from point index since sparkline points are hourly VWAP); nothing rendered
  until hover so tables stay clean. Same gap-interpolation and trend coloring
  (green/red/slate by 24h % change) as today. Used in Market Movers, Continue
  Tracking, Trends, and the Analyzer.

## Integration & removals

- Workspace `Cargo.toml`: remove `plotters`; `ultros/Cargo.toml`: remove
  `plotters-svg`; `ultros-app/Cargo.toml`: remove `leptos-chartistry`.
- `ultros/src/web/item_card.rs`: replace the plotters backend dance with
  `ultros_charts::render_sale_history_svg(...) -> String`; resvg pipeline unchanged.
- `price_history_chart.rs`: shrinks to wiring (signals → component props, toggle
  buttons, stats strip markup).
- `sparkline.rs`: becomes a re-export / thin wrapper of the crate component.
- Any new user-visible strings (tooltip labels etc.) get leptos-i18n keys in **all
  7 locales** per CLAUDE.md.

## Error handling

- Empty or insufficient data renders a deliberate "no data" scene — no panics, no
  unwraps on empty slices.
- The PNG path keeps returning `anyhow::Result`; the axum handler maps errors as today.

## Testing

- Unit tests for every `data/` transform and the scales/tick generation.
- A golden-SVG snapshot test of a fixed dataset to catch unintended visual drift.
- A PNG smoke test in `ultros` that renders sample sales and asserts a decodable,
  non-empty PNG.

## Rollout — three PRs

1. **Crate core + PNG path.** New ultros_charts internals, `item_card.rs` swap,
   plotters/plotters-svg removed.
2. **Web price chart.** `<PriceHistoryChart>` on the item page, leptos-chartistry
   removed.
3. **Interactive sparklines.** `<Sparkline>` component, all four call sites migrated.
