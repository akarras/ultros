# Comparison: small multiples, shared crosshair, world filter

**Status:** design
**Date:** 2026-07-26
**Sequence:** 3 of 4 (chart revamp). Depends on spec 1; extends spec 2's toolbar.

## Problem

Comparing worlds is the question the item page is most often opened to answer,
and the chart is bad at it:

- **A world-scope page can only ever show one series.** `available_group_levels`
  returns `[World]` for a world scope, so the grouping control hides and there is
  nothing to compare against.
- **Overlaid lines at different price levels waste the axis.** A world trading at
  380k and one at 420k stack into two flat ribbons with all the variation
  squashed, because the y-domain is dominated by the offset between them rather
  than the movement within them.
- **The palette runs out at twelve.** A region-scope page grouped by world can
  produce eighty series; the legend already truncates at ten and colors wrap.
- **Three of spec 2's four modes are single-series**, so they are unusable for
  comparison by construction.

A grid of small multiples fixes all four at once — but only if the pointer
synchronises across cells. Without that, comparing cell to cell means reading
one, remembering it, and moving on, which is exactly what charts exist to avoid.

## Goals

- A grid view where each series is its own small chart, in any spec 2 mode.
- One pointer position driving every cell simultaneously.
- A world filter that scales to a region's worth of worlds.
- Make overlay usable for series at different price levels.

## Non-goals

- No arbitrary cross-scope comparison (picking Gilgamesh and Balmung and an
  Aether average as three explicit series). The page's own scope picker stays
  authoritative for what data is loaded; this spec only changes how that data is
  divided and drawn. A "compare tray" was considered and rejected as redundant
  with the existing scope picker.
- No new fetching. Grid cells are the series of the *current* grouping, not
  always worlds — a region-scope page grouped by datacenter produces one cell per
  datacenter. So switching between overlay and grid never changes the request,
  and spec 1's per-series payload already contains everything here.

## Design

### Shared time index

The server returns one bucket list per series, and a series with no sales in a
bucket has no row for it. So cells do not have identical bucket lists, and a
naive per-cell hover index would point at different times in different cells.

The client builds a **union time index** once per `PriceSeries`: the sorted set
of all bucket timestamps across all visible series, with each series mapped onto
it by position, gaps as `None`. This is a generalisation of what
`HoverModel::buckets` already does — it holds `series_values: Vec<Option<..>>`
keyed by bucket, which is exactly the right shape. The change is that the index
becomes shared state owned above the cells rather than derived per chart.

Because every series in a response came from one query with one bucket width,
the union index is exact — no interpolation or snapping.

### Shared crosshair

One `hover_index: RwSignal<Option<usize>>` into the union index, owned by the
grid container and read by every cell. Pointer movement over any cell resolves
x → union index once, at the container, and every cell re-renders its own
crosshair and dot from the same value.

`HoverModel::nearest_index` already does the resolution and is already tested;
it operates on the union index instead of a per-chart one.

Because all cells share a time domain and a pixel width, x → index resolves
identically in every cell — so the crosshair lines up visually across the grid,
which is the property that makes the whole view work.

The tooltip stays a single element at the container level, showing every series'
value at the hovered bucket, rather than one tooltip per cell.

### Grid layout

- Column count from container width, targeting a cell width of ~220–320px.
- Cells sorted by series name by default (stable across refetches), with an
  option to sort by change over the visible window.
- **Shared y-domain across cells by default.** Per-cell auto-scaling makes each
  cell maximally readable but makes cells incomparable, which defeats the point;
  a shared domain means cell height is meaningful across the grid. Per-cell
  scaling is available as an option for the case where one outlier world flattens
  everything else.
- Cell cap of 24, with the remainder collapsed into a "+N more" affordance that
  opens the world filter. Beyond ~24 cells nothing is legible anyway, and the cap
  bounds the node count.
- Each cell labels itself, so color is no longer load-bearing for identification.
  Cells past the twelfth reuse hues without ambiguity.
- Volume lane is omitted in grid cells — at cell size it is noise. It stays in
  overlay.

### Indexed percent change

An **Index to % change** toggle in the Overlays popover, rebasing every series to
0% at the first bucket in the visible window.

Available in overlay view only. In grid, each cell already has its own frame, so
the offset problem the toggle solves does not exist there; offering it would just
be a second way to express the same thing.

With it on, the y-axis is percent, the market-average overlay is meaningless and
is disabled with a reason, and the caption line says so.

### World filter

A chip opening a searchable multi-select popover, listing every world in the
current scope grouped by datacenter, with select-all and select-none per group.

It drives the **existing `hidden_series` signal** rather than introducing a
parallel concept — it is the same operation as clicking a legend chip, scaled to
a list you can search. Legend clicks and the filter stay in sync automatically
because they write the same state.

Filtering is purely client-side and instant: it changes visibility, not grouping,
so it never triggers a refetch. It does affect the axes, since hidden series are
already excluded from the domain calculation today.

The chip shows a count badge (`8 / 11`) so a non-default filter is visible
without opening the popover — a hidden filter that silently omits data is a
correctness hazard, not just a UX one.

Icon: `TbFilterOutline`, already vendored.

### View toggle

Two-item icon-only segmented group in the toolbar slot spec 2 reserved:

| View | Icon |
|---|---|
| Overlay | `LuChartNoAxesCombined` |
| Grid | `LuLayoutGrid` |

Grid is available in every mode — it is the only control in spec 2's interaction
matrix that is unconditionally available, and it is what rescues the
single-series modes. A wall of per-world candlesticks under one shared crosshair
is the thing that makes Candles and Density worth having at all.

When the user picks Candles or Density while in overlay with a multi-series
grouping, the hint surfaced by spec 2 should offer switching to grid as its
action, rather than just explaining the limitation.

## Testing

`ultros-charts`:

- The union index over three series with disjoint gaps contains every distinct
  timestamp exactly once, sorted, with each series mapped to the right positions
  and `None` in its gaps.
- A series whose buckets are a strict subset of another's maps without shifting.
- Shared y-domain across cells equals the domain of the union of visible series,
  and excludes hidden ones.
- Cell cap: 40 series produce 24 cells plus an overflow marker.

Frontend:

- `hover_index` set from a pointer event over cell 5 is observable by cells 1
  and 12 in the same tick.
- Hiding a series via the filter and via a legend click produce identical
  `hidden_series` state.
- Hiding every series produces the empty-state card and the legend still offers
  un-hiding — the existing
  `hiding_every_series_yields_the_no_data_card_but_keeps_metadata` guarantee must
  survive the grid refactor.
- The filter count badge reflects a non-default filter.
- Switching overlay → grid preserves mode, grouping, filter and time window.
- `% change` is offered in overlay and absent in grid.

## Risks

- **Grid at 24 cells × 2,000 buckets is a lot of marks.** Spec 1's batching makes
  it tractable, but the cell cap and the omission of the volume lane are both
  load-bearing, not cosmetic. If it is still slow, the next lever is reducing
  bucket resolution for grid cells specifically — a cell 260px wide cannot show
  2,000 buckets meaningfully anyway.
- **Shared y-domain plus one wild world** flattens every other cell. The per-cell
  scaling option is the escape hatch; if users reach for it constantly, the
  default is wrong.
- **The union index grows with series count.** At region scope grouped by world
  with 80 series it is fine as a structure, but the `Vec<Option<..>>` per bucket
  is 80 wide. Worth measuring before assuming.

## Open questions

- **Indexed % change was proposed but never explicitly confirmed.** It is
  specified here as an overlay-only toggle because overlay is unusable without
  something like it, but it is the one item in this spec carried on my judgement
  rather than an explicit decision. Cutting it costs nothing structurally.
- Should grid cells be clickable to promote that world to a focused single
  chart? Natural, but adds navigation state; deferred.
- Sort-by-change needs a defined window — the visible range, presumably, but that
  makes sort order shift while dragging the timeline slicer. May need debouncing
  or an explicit "sort now" rather than live re-sorting.
