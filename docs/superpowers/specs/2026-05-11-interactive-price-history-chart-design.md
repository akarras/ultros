# Interactive Price History Chart — Design

**Date:** 2026-05-11
**Status:** Awaiting user review
**Scope:** Replace the plotters-canvas in-browser scatter chart on the item view with an interactive, SVG-based chart. Leave server-side PNG generation on plotters.

---

## Why

The current sale-history chart on the item view is a rasterized plotters render to a `<canvas>`. It's a static picture — no hover details, no quick-filtering, no per-series toggling. Every theme or resize event triggers a full redraw. For a price page where users want to read individual sales, that's the wrong primitive.

## Non-goals

- Replacing the PNG download generator at [ultros/src/web/item_card.rs](../../ultros/src/web/item_card.rs). That stays on plotters — it's a server-rendered raster, not interactive, and the existing pipeline (plotters → SVG → resvg → PNG) works fine.
- Removing the `ultros-charts` crate. The PNG path still depends on it.
- Touching the `linregress`-style stats anywhere else in the app.

## Library choice: `leptos-chartistry`

Picked over hand-rolled SVG and over JS-interop options (ECharts, uPlot, Chart.js, Plotly):

- Pure Leptos. No JS bundle, no `wasm-bindgen` glue per chart event, no SSR/hydrate hand-wringing.
- SVG output. Themes via CSS custom properties and updates reactively without a redraw cycle.
- Built-in hover tooltip + nearest-point detection, legend, multi-series.
- Reactive to Leptos signals — the existing `Signal<Vec<SaleHistory>>` input shape works directly.

Trade-off accepted: chartistry lacks first-class brush-zoom. We replace it with discrete time-range chips, which suits the 200-sale data cap better than a brush anyway.

## Component shape

Replace [ultros-frontend/ultros-app/src/components/price_history_chart.rs](../../ultros-frontend/ultros-app/src/components/price_history_chart.rs) wholesale. Public API unchanged:

```rust
#[component]
pub fn PriceHistoryChart(#[prop(into)] sales: Signal<Vec<SaleHistory>>) -> impl IntoView
```

Internal layout (top to bottom):

1. **Header row**: title (`"<Item> — Sale History"`) on the left, controls on the right.
   - Controls: outlier-filter toggle (existing) + time-range chips (new).
2. **Stats strip**: one muted line — `n` sales · median · mean · min · max. Hidden when n=0.
3. **Chart body**: the chartistry SVG. Aspect-ratio sized, no magic min-heights.
4. **Legend**: chartistry's built-in legend, beneath the chart.

The outer wrapper currently in [item_view.rs:708](../../ultros-frontend/ultros-app/src/routes/item_view.rs:708) (`<div class="panel p-6 ...">`) collapses into the component itself — the call site becomes `<PriceHistoryChart sales=filtered_sales />` with no surrounding panel div. The component owns its own panel chrome.

## Time-range chips

Chips above the chart: **24h · 7d · 30d · All**. Default: **All** (matches current behavior).

Filter is applied inside the component, after the prop is read, before chartistry sees the data. Selected chip is stored in a local `RwSignal<TimeRange>`. Filtering is a `Memo` over `(sales, range)`.

Chips that would produce zero points are still clickable — the empty state ("No sales in this window") replaces the chart body so users understand why and can switch back.

## Stats strip

Computed from the *filtered* sale list (after both the time-range chip and the outlier toggle):

- **n**: count
- **VWAP**: `Σ(price × qty) / Σ(qty)`, rounded integer — same value the line draws
- **median**: middle of sorted prices
- **min / max**: bookends

Rendered as `n sales · VWAP 1.2K · median 1.1K · min 800 · max 2.4K`, using the existing `short_number` helper logic (lift it from [ultros-charts/src/lib.rs:24](../../ultros-frontend/ultros-charts/src/lib.rs:24) into the component or a small util module). The arithmetic mean is dropped — VWAP is the better central-tendency number for trade data and showing both invites confusion.

Hidden when `n == 0`; replaced by a one-line "No sales in this window" muted notice.

## Series grouping

Keep the current "world → DC → region rollup" rule from [map_sale_history_to_line](../../ultros-frontend/ultros-charts/src/lib.rs:407):

- More than one world but one DC → series per world
- More than one DC but one region → series per DC
- Otherwise → series per region

Extract this into a small pure function inside the component module (call it `group_sales_by_locale`). It does **not** need to live in `ultros-charts` — the plotters version there can stay using its own copy, and we avoid an awkward shared dep with WASM-only types.

## Visual elements

The overlay set is chosen to match conventions from financial trade-tape visualizations, adapted for FFXIV's heavy-tailed price distribution.

- **Series**: scatter only (no connecting lines — multi-world overlap would be visual noise; the trend overlays carry that signal).
- **Point size**: proportional to `quantity`, clamped (chartistry supports point-size from data; if not, use 3 fixed sizes by quantity bucket: 1, 2–10, 11+). This is the standard trade-tape convention — volume-scaled markers.
- **VWAP line** (new, always on): volume-weighted average price line drawn across the current time window. Computed as `Σ(price × qty) / Σ(qty)`. Drawn as a single solid line in a distinct accent color (brand-300 or similar) at moderate opacity. VWAP is the financial-standard central-tendency line for trade history because it answers "what did people actually pay on average," weighted by transaction size. Strictly preferable to median or arithmetic mean for this dataset.
- **IQR band** (always on, dimmed when outlier filter is active): translucent horizontal band between Q1 − 2.5·IQR and Q3 + 2.5·IQR (same formula as today's `get_iqr_filter`). Drawn as a chartistry ref-line pair or a thin filled rect underlay. Chosen over Bollinger bands (mean ± 2σ) because FFXIV market data is heavy-tailed (vendor floors, RMT outliers, whale buys) and stdev bands misbehave on non-normal distributions. When the outlier filter is on, opacity drops further so the band reads as a quiet "this is the typical zone" hint rather than a competing element.
- **Trendline** (always on): least-squares fit, single line across all series, drawn faint. Same math as today. Provides directional signal across the window.
- **Colors**: read `--color-text`, `--color-outline`, `--color-brand-*` from the document element on mount and on theme-change. Series palette: cycle through brand + accent CSS vars (look up what's defined in [style/tailwind.css](../../style/tailwind.css) — the current chart only reads two vars; we'll need ~6 palette slots plus distinct colors for the VWAP line and trendline).

**Why no toggles for VWAP / IQR / trendline?** The time-range chips and outlier filter already give plenty of control surface. Financial trading UIs default to showing standard overlays and tuck visibility controls behind a kebab menu — adding three separate toggles for v1 is clutter. If users ask, a single "Overlays" disclosure can be added later.

## Hover behavior

Chartistry's built-in hover gives a tooltip with the nearest point. We populate it with:

- Series name (world / DC / region)
- Sold date (formatted like the x-axis tick: `YYYY-MM-DD HH:MM` if range < 2d, else `YYYY-MM-DD`)
- Price per item (formatted with `Gil` component if practical; otherwise plain `i32` with thousands separators)
- Quantity

No crosshair guide lines for now — chartistry's default hover affordance is enough.

## Layout / CSS

- Container: existing `panel` class, padding tightened to `p-4 md:p-6`.
- Aspect ratio: `aspect-[16/9]` on the chart body wrapper, capped at `max-h-[520px]` to prevent ultra-wide screens from stretching.
- Header row: `flex flex-wrap items-center justify-between gap-3` so chips and toggle wrap to a second line on narrow screens.
- Chips: reuse existing button styles; selected chip uses `bg-brand-500/20 border-brand-400` to match the world-button pattern in [item_view.rs:46-56](../../ultros-frontend/ultros-app/src/routes/item_view.rs:46).
- Stats strip: `text-sm text-[color:var(--color-text)]/70 tabular-nums`.
- Mobile: chart stays full-width, chips wrap, stats strip wraps to two lines if needed.

## Dependencies

Add to [ultros-frontend/ultros-app/Cargo.toml](../../ultros-frontend/ultros-app/Cargo.toml):

```toml
leptos-chartistry = { version = "0.2", default-features = false }
```

(Exact version pinned to whatever supports leptos 0.8.14 — verify during implementation.)

Remove from the same file:

```toml
plotters-canvas = "0.3"
```

`ultros-charts` stays — still used by the PNG generator.

## SSR safety

Chartistry renders SVG, so it works under SSR without `web-sys` shenanigans. The chart appears in initial HTML; hydration only adds the hover interactivity. This is an improvement over today, where the canvas is blank until WASM loads.

The `chart_colors` `Memo` that reads `window.getComputedStyle` must remain `#[cfg(feature = "hydrate")]`-gated, returning sensible fallback colors during SSR. The fallback path (already in the current code, lines 88–90) is fine.

## Accessibility

- `role="img"` on the SVG with an `aria-label` summarizing: `"Sale history scatter plot, n sales over <range>"`.
- Chips are real `<button>` elements with `aria-pressed`.
- Toggle keeps its existing keyboard behavior.
- Tooltip text is also reflected in a visually-hidden live region so screen-reader users get hover info on focus (chartistry support for this is partial — if it doesn't expose it, ship without and add a follow-up task).

## What gets deleted

- `plotters-canvas` dependency.
- The `__parse_css_rgb` helper in [price_history_chart.rs:45-67](../../ultros-frontend/ultros-app/src/components/price_history_chart.rs:45) — replaced with chartistry's CSS-native styling.
- The `chart_colors` memo as currently shaped (replaced by a leaner version that just exposes the brand palette to chartistry as a list of color strings).
- The `min-h-[440px]` / `min-h-[480px]` skeleton swap dance — chartistry shows axis chrome immediately even with no data.

## Risk / fallback

If `leptos-chartistry` turns out to be incompatible with Leptos 0.8.14 (the latest release I'm aware of targets 0.7), the fallback is **hand-rolled SVG in Leptos**. Same component API, same surrounding layout, same chips/stats/legend — just an in-tree `Scatter`/`Tooltip` impl in ~200 lines of Leptos SVG. The design doesn't depend on chartistry-specific features; everything described above is achievable in plain Leptos SVG. Decision point during implementation: if `cargo add` fails on version constraints, switch to hand-rolled without revisiting design.

## Testing

- `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings` (per CLAUDE.md).
- Manual: item page in browser, verify hover tooltip, chips filter correctly, outlier toggle still works, theme switch updates colors live, "Download PNG" button still produces a valid plotters-rendered image.
- Verify SSR HTML contains the chart SVG (view-source on the item page before WASM hydration).

## Out of scope (future work)

- Brush-zoom for arbitrary time windows.
- Linking the chart to the sale history table (click point → scroll to row).
- Per-series legend toggle persistence (remember last-deselected series across navigations).
- A dedicated "Overlays" menu (kebab) to toggle VWAP / IQR / trendline visibility — only add if users ask.
- Time-bucketed VWAP (e.g. one VWAP point per hour) instead of a single window-wide VWAP line.
