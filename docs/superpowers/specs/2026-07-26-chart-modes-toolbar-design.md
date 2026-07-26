# Chart modes and the icon toolbar

**Status:** design
**Date:** 2026-07-26
**Sequence:** 2 of 4 (chart revamp). Depends on spec 1. Spec 3 extends the
toolbar built here.

## Problem

The chart renders exactly one thing: a VWAP line per series with raw sales
scattered behind it. That is the right default and the only mode that overlays
many series legibly, but it cannot answer questions the data supports:

- *How volatile was this?* — the line hides intra-bucket range entirely.
- *What is a normal price, and how wide is the spread?* — a single VWAP number
  per bucket says nothing about dispersion.
- *Is there an undercut war?* — a bimodal price distribution looks identical to
  a unimodal one once averaged.

Meanwhile the control surface is already three stacked rows of chips and a
slicer, so anything added has to make it tighter rather than taller.

## Goals

- Three new render modes, each answering one of the questions above.
- A denser toolbar that fits the new controls in less vertical space than today.
- Make the mode/control interactions legible rather than surprising.

## Non-goals

- No data-fetching changes. Spec 1's `PriceSeries` already carries every column
  these modes need; this spec only renders columns that are already arriving.
- No grid/small-multiples view and no world filter — spec 3.
- No milestones — spec 4.

## Design

### The four modes

```rust
pub enum ChartMode { Price, Candles, Range, Density }
```

**Price** (default, unchanged behaviour). VWAP polyline per series from
`gil / units`, area fill when a single series is visible, raw dots when spec 1
supplied them. The only mode that overlays many series legibly.

**Candles.** Per bucket: body from `open`/`close`, wick from `high`/`low`,
colored by direction. Single series only.

Thin buckets are the honesty problem here: a day with two sales produces a
candle whose open and close are just "the two sales", and the shape implies
structure the data does not have. Mitigation: buckets with `sales < 3` render as
a wick-only tick with no body. That reads as "range known, direction unknown",
which is exactly true, and it means a sparse item looks sparse instead of
looking like a trending market.

**Range.** `p50` polyline, `p25`–`p75` ribbon, faint `low`–`high` ribbon behind
it. Uses the existing `Node::Area`. Degrades gracefully on thin buckets — the
band simply narrows. Supports up to two visible series before the ribbons turn
to mud; beyond two, the legend disables further un-hiding with a reason.

**Density.** A time × price grid, each cell shaded by sale count. Requires a
second aggregate — see "Density query" below.

### Mode/control interaction

Three of four modes are single-series by nature, so the mode choice changes what
the other controls can do. This is the confusing moment, more than any icon
legibility question, so affected controls are **disabled with a reason**, never
hidden — a control that vanishes reads as a bug.

| Mode | Group by | Quantity lane | Series |
|---|---|---|---|
| Price | all levels | yes | many |
| Candles | forced to one | yes | 1 |
| Range | all levels | yes | up to 2 |
| Density | forced to one | no | 1 |

"Forced to one" means: the grouping control stays enabled, but selecting a level
that yields more than one series shows only the first and surfaces a hint. Spec
3's grid view is what genuinely resolves this, and the hint should say so.

### Density query

Density is the one mode spec 1's payload cannot serve, because it needs price
bins, not price aggregates. It gets its own endpoint:

`GET /api/v1/price_density/{world}/{itemid}` with the same `from`/`to`/`hq`
params plus `price_bins` (default 32) and the same bucket ladder.

```sql
SELECT
    toStartOfInterval(sold_date, INTERVAL {bucket} SECOND) AS bucket,
    floor((price_per_item - {lo}) / {bin_width})           AS price_bin,
    count()                                               AS n
FROM sales
WHERE item_id = ? AND world_id IN (?) AND sold_date >= ? AND sold_date < ?
GROUP BY bucket, price_bin
```

`{lo}` and `{bin_width}` come from a cheap prior `min`/`max` over the same
predicate, or are reused from the `PriceSeries` response the client already
holds. Payload is a fixed `buckets × price_bins` grid — a few thousand numbers
regardless of whether the item has ten thousand sales or ten million. At full
history density is the **cheapest** mode to render, not the most expensive.

Caching and the LOD ladder work exactly as in spec 1.

### Rendering budget

Candles and density cells are emitted through spec 1's `Node::Path`: one node
per fill color rather than one per mark. 2,000 candles become two nodes (one up,
one down) plus one for wicks. Density becomes one node per opacity step, with
the ramp quantised to ~8 steps for this reason.

### Color

The existing `Theme::palette` is categorical — twelve hues chosen to be
distinguishable, which is the wrong tool for density. `Theme` gains a
`density_ramp: Vec<Color>`, a sequential ramp holding lightness order, plus
`candle_up` / `candle_down`.

Candle colors must not be the naive red/green pair: that is the one palette
choice that fails for the most common form of color blindness, and direction is
the entire message of a candle. Use a diverging pair that separates on lightness
as well as hue, and verify the up/down distinction survives a greyscale render —
the item card PNG makes that easy to check.

### Toolbar

`price_history_chart.rs`'s three chip rows collapse into one `ChartToolbar`
component: a horizontally scrollable flex row of segmented groups and chips.

Layout, left to right:

1. **Mode** — icon-only segmented group, 4 items.
2. *(slot reserved for spec 3's view toggle)*
3. **Group by** — single dropdown chip showing its current value as text.
4. *(slot reserved for spec 3's world filter)*
5. **Overlays** — chip with a count badge, opening a popover containing market
   average, trendline, quantity lane.

Icons, all from the already-vendored `icondata` (no new dependency):

| Control | Icon |
|---|---|
| Price | `TbChartLineOutline` |
| Candles | `TbChartCandleOutline` |
| Range | `TbChartAreaLineOutline` |
| Density | `TbChartGridDotsOutline` |
| Overlays popover | `TbAdjustmentsHorizontalOutline` |
| Region | `TbStack2Outline` |
| Datacenter | `TbCirclesOutline` |
| World | `TbPointFilled` |

The group-by control keeps today's auto-collapse, which already does exactly
what is wanted: `available_group_levels` returns `[World]` for a world scope and
`[Datacenter, World]` for a datacenter scope, and `ColorByControl` wraps itself
in `<Show when=options.len() > 1>`. So the chip disappears on a Gilgamesh page
and offers exactly two levels on an Aether page, with no new logic. The
grouping icons appear beside their labels inside the dropdown menu, where they
have text next to them and the 16px legibility problem does not arise.

### Caption line

The toolbar is icon-only, so the resolved state is spelled out once beneath the
chart, replacing the current `StatsStrip`:

> Price line · grouped by World · 1,204 sales · avg 41.2k · median 39.8k

This is what makes an all-icon toolbar viable. It works on touch, where tooltips
never fire; it is what a screen reader announces for the chart region; and it
means no icon has to carry its meaning alone. It costs one row and removes
another, so the chart is net shorter than today.

### Tooltips and i18n

Tooltips are **not** the primary affordance. Every icon-only button needs an
`aria-label`, which is an i18n key in all seven locales regardless — so a
visible label or caption costs zero additional keys, while a tooltip costs a
second key per control on top of the aria-label. Tooltips are added only where
a control has a non-obvious consequence (the disabled-with-reason states).

Per `CLAUDE.md`, every new string lands in all seven locale files
(`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) with a real translation, using
`snake_case` keys under a `chart_mode_*` / `chart_toolbar_*` prefix. Expansion
and patch names are spec 4's concern and deliberately avoid new keys.

## Testing

`ultros-charts`, per mode, from a shared `PriceSeries` fixture:

- Candles emit one body per bucket with `sales >= 3` and a wick-only tick below
  that threshold.
- An all-equal-price fixture yields zero-height bodies that still render (the
  `max(height, 1.2)` floor), rather than disappearing.
- Range emits exactly three area layers plus one polyline, ordered so the median
  draws last.
- Density emits at most one node per quantised opacity step, and cell count
  equals populated `(bucket, bin)` pairs — not `buckets × bins`.
- Every mode with an empty series produces the "No recent sales" card.
- Node count for a 2,000-bucket candle fixture stays in single digits — the
  regression guard for batching.
- Greyscale check: `candle_up` and `candle_down` differ in luminance by a
  documented minimum.

Frontend:

- Switching mode preserves the time window and grouping.
- Selecting a multi-series grouping in Candles or Density surfaces the hint and
  renders one series rather than erroring.
- The group-by chip is absent on a world-scope page and shows two options on a
  datacenter-scope page.
- The caption line matches the active mode, grouping, and stats.

## Risks

- **Density is the least familiar chart here.** It is the mode most likely to
  confuse, and the one whose value is hardest to explain in a caption. If it
  tests badly it can ship behind the same toolbar without being the default, or
  be dropped entirely without affecting the other three.
- **Icon-only mode selection was chosen over labels.** The caption line is the
  mitigation, and it is load-bearing — if the caption is cut for space, this
  decision should be revisited rather than shipped as-is.
- **Candle semantics on sparse data** remain a judgement call even with the
  three-sale rule. Worth revisiting against real items once it is live.

## Open questions

- Should Range support two series, or is one simpler and enough? Two is
  specified; if the overlap reads badly in practice, dropping to one removes the
  legend's disable-with-reason case entirely.
- Should mode persist across items (localStorage) or reset to Price each time?
  Leaning reset, so a shared link and a fresh visit agree, but a returning power
  user may disagree.
