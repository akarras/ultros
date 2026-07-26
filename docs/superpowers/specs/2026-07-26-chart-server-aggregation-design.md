# Server-side sale aggregation and level-of-detail rendering

**Status:** design
**Date:** 2026-07-26
**Sequence:** 1 of 4 (chart revamp). Blocks specs 2 and 3.

## Problem

The item page chart fetches up to 10,000 raw sales from
`/api/v1/extended_history/{world}/{itemid}`, buckets them in WASM, and emits one
`Node::Circle` per sale into the scene. Three consequences:

1. **History is capped by row count, not by time.** The 10k limit is ordered by
   recency, so a popular item at region scope may only reach back a few days.
   Long-run market history is unreachable at any scope.
2. **The DOM is the bottleneck before the database is.** 10,000 sales means
   10,000 SVG elements. The chart is already at its rendering ceiling, and that
   ceiling is lower than the data ceiling.
3. **Bucketing is recomputed in the browser** on every model rebuild — every
   resize step, every toggle, every grouping change.

ClickHouse already holds a full-history mirror of `sale_history` in `sales`,
ordered by `(item_id, hq, world_id, sold_date, pg_id)`. A per-item aggregate is
a prefix scan — the cheapest query shape the table supports. Nothing about the
current design uses that.

## Goals

- Serve pre-bucketed price/volume series from ClickHouse, so payload size is a
  function of the requested window rather than the item's popularity.
- Make full history reachable at every scope.
- Cut scene node count so the chart stays responsive at any history depth.
- Return the aggregate columns that specs 2 and 3 need (OHLC, quantiles), even
  though nothing renders them yet.

## Non-goals

- No new chart types, no UI changes, no new controls. The chart looks the same
  after this spec; it is simply fed differently. New modes are spec 2.
- No new rollup table and no backfill. We query `sales` directly and cache. See
  "Why not a rollup table".
- The existing `/api/v1/extended_history` endpoint stays. The item card PNG path
  and any external consumers keep working unchanged.

## Design

### Wire types

New in `ultros-api-types/src/price_series.rs`, re-exported from `lib.rs`
alongside `CompactSale`:

```rust
pub struct PriceSeries {
    /// Bucket width the server actually chose, so the client labels axes
    /// consistently without re-deriving it.
    pub bucket_seconds: i64,
    /// Grouping the server aggregated at, echoing the request.
    pub group: SeriesGroup,
    /// Time domain actually covered by the data (not the requested range).
    pub from: NaiveDateTime,
    pub to: NaiveDateTime,
    pub series: Vec<PriceSeriesEntry>,
    /// Raw sales, present only when the window is small enough to draw them.
    /// See "Level of detail".
    pub raw: Option<Vec<CompactSale>>,
}

pub struct PriceSeriesEntry {
    /// Selector id at the requested grouping (world / datacenter / region id).
    pub id: i32,
    pub buckets: Vec<PriceBucket>,
}

pub struct PriceBucket {
    /// Bucket start, UTC, aligned to absolute time like today's buckets.
    pub ts: NaiveDateTime,
    pub open: i32,
    pub high: i32,
    pub low: i32,
    pub close: i32,
    /// Sum of price_per_item * quantity; VWAP is gil / units.
    pub gil: i64,
    pub units: i64,
    pub sales: u32,
    pub p25: i32,
    pub p50: i32,
    pub p75: i32,
}

pub enum SeriesGroup { Region, Datacenter, World }
```

`gil` and `units` are carried rather than a precomputed `vwap` so the client can
re-derive VWAP over any subset of buckets (the timeline slicer needs this) with
exact arithmetic.

### Grouping happens server-side

The request carries the grouping level and the server aggregates at it. This is
a deliberate change from today, where the client regroups locally.

The reason is quantiles. `min`, `max`, `gil`, `units` and `sales` are all
re-aggregatable client-side — a datacenter's `high` is the max of its worlds'
`high`s. Quantiles are not: a datacenter's p50 is not any function of its
worlds' p50s. `open`/`close` are re-aggregatable only if bucket rows also carry
the timestamps those prices came from. Rather than ship two classes of column
with different composition rules and an easily-violated invariant, the server
groups.

Cost: changing the group-by control becomes a fetch instead of an instant
recompute. Mitigated by the response being small and cached on both sides, and
by the control already being hidden entirely on world-scope pages.

Hiding a series via the legend or the world filter stays purely client-side and
stays instant — it is a visibility flag, not a regrouping.

### Endpoint

`GET /api/v1/price_series/{world}/{itemid}`

| Param | Type | Default | Notes |
|---|---|---|---|
| `from` | unix seconds | earliest available | |
| `to` | unix seconds | now | |
| `bucket` | seconds | derived from span | clamped to a known ladder |
| `group` | `region\|datacenter\|world` | narrowest valid for scope | |
| `hq` | `any\|hq\|nq` | `any` | replaces the client-side HQ filter |

`{world}` resolves through `world_cache.lookup_value_by_name` and
`get_all_worlds_in`, exactly as `extended_sale_history` does today — so world,
datacenter and region scope names all work with no new resolution logic.

When `bucket` is absent the server derives it from the span using
`ultros_charts::data::buckets::bucket_seconds`. `ultros` already depends on
`ultros-charts` (the item card PNG path uses it), so this is a direct reuse and
guarantees the server and the client agree on bucket boundaries. When `bucket`
is supplied it is snapped to the same ladder rather than honoured verbatim, so
a hand-crafted request cannot ask for a million buckets.

Response is capped at 20,000 buckets across all series. Exceeding it widens the
bucket one ladder step and retries, rather than truncating — truncation would
silently drop the oldest data, which is the data the user asked for.

### Query

New `price_series` in `ultros-clickhouse/src/queries.rs`, following the shape of
the existing query functions:

```sql
SELECT
    world_id,
    toStartOfInterval(sold_date, INTERVAL {bucket} SECOND) AS bucket,
    argMin(price_per_item, sold_date)          AS open,
    max(price_per_item)                        AS high,
    min(price_per_item)                        AS low,
    argMax(price_per_item, sold_date)          AS close,
    sum(total_gil)                             AS gil,
    sum(quantity)                              AS units,
    count()                                    AS sales,
    quantileExact(0.25)(price_per_item)        AS p25,
    quantileExact(0.50)(price_per_item)        AS p50,
    quantileExact(0.75)(price_per_item)        AS p75
FROM sales
WHERE item_id = ?
  AND world_id IN (?)
  AND sold_date >= ? AND sold_date < ?
  {hq_predicate}
GROUP BY world_id, bucket
ORDER BY bucket
```

Notes:

- `item_id` is filtered first and there is **no join**, keeping the read on the
  table's primary key prefix. The `sparklines_batch` comment in `queries.rs`
  documents what happens when a rollup is joined unfiltered; this query must not
  repeat that.
- `total_gil` is a MATERIALIZED column, so `sum(total_gil)` reads a stored
  column rather than multiplying per row.
- The query returns per-world rows always. Roll-up to datacenter or region
  happens in Rust for the composable columns; **when `group` is not `world`, the
  `GROUP BY` key changes to the mapped selector id** so quantiles are computed
  at the right level. The world→datacenter→region mapping is passed in as a
  literal `CASE`/`transform` built from `WorldHelper`, avoiding a join.
- No `FINAL`. `sales` is a `ReplacingMergeTree` and duplicate rows are exact
  duplicates of the same sale; at aggregate scale an unmerged duplicate shifts a
  bucket VWAP imperceptibly, and `FINAL` on a full-history scan is expensive.
  This is a deliberate accuracy-for-cost trade and should be noted in the
  function's doc comment.

### Caching

Two layers, no new infrastructure:

1. **In-process TTL cache** keyed by the full parameter tuple
   `(item_id, scope, from, to, bucket, group, hq)`. Windows ending at "now" are
   normalised — `to` snaps down to the current bucket boundary — so live views
   share cache entries instead of generating a unique key per request.
   TTL: one bucket width, floored at 60s and capped at 1h.
2. **`Cache-Control`** on the response with the same value, so the browser and
   any CDN in front absorb repeats.

Closed windows (a `to` in the past) are immutable and get a long TTL.

### Level of detail

The `raw` field is populated only when the server's `count()` over the window is
at or below `RAW_SALE_LIMIT` (2,000). Above that the field is `None` and the
chart draws buckets only.

This makes raw dots a zoomed-in affordance rather than a default. Zooming the
timeline slicer to a narrow window naturally re-crosses the threshold and brings
the dots back, which is the behaviour a user would expect anyway.

### Scene: batched marks

Add one variant to `ultros_charts::scene::Node`:

```rust
/// Pre-serialized path data. Lets a layout emit N marks sharing one fill or
/// stroke as a single node instead of N nodes.
Path {
    d: String,
    fill: Option<Color>,
    stroke: Option<Stroke>,
}
```

`svg.rs` gains one serialization arm; `components.rs` gains one Leptos arm. Both
are mechanical.

In this spec the only consumer is the raw-dot layer, which becomes one `Path`
per series instead of one `Circle` per sale. Spec 2's candles and heatmap cells
are the variant's real payoff, but adding it here means spec 2 has nothing to
change in the renderer.

`Node::Circle` stays — the hover layer draws a handful of them and per-node
reactivity is correct there.

### Chart layout changes

`build_price_history_chart` currently takes `&[SaleHistory]` and calls
`group_sales_by_level` and `vwap_buckets`. It changes to take a `PriceSeries`.

Consequences inside `ultros-charts`:

- `data::buckets::{vwap_buckets, volume_buckets_from_points}` lose their callers
  from the price-history path. `bucket_seconds` is retained and now shared with
  the server. `sparkline.rs` keeps using what it needs.
- `data::grouping::group_sales_by_level` is no longer called by the web chart,
  because grouping is a server concern now. It stays for the item card PNG path
  until that path is migrated (see below), and `available_group_levels` stays —
  it drives which grouping the UI may request.
- `data::outliers::filter_outliers` operates on raw sales. With buckets, outlier
  filtering has to move into the query as a price predicate. **v1 keeps the
  toggle working only when `raw` is present**, and hides it otherwise, rather
  than silently changing what the toggle means. Making outlier filtering a
  server-side predicate is deferred and noted as an open question.

The item card PNG path (`/itemcard/{world}/{id}`) migrates too. It is not
affected by the problems this spec solves — fixed size, bounded window — so
leaving it alone would have been preferable, but it calls
`build_price_history_scene`, which shares `build_price_history_chart`'s
signature. Keeping it on raw sales would mean maintaining two layout code paths
that must produce identical output, which is exactly the drift the crate's
module docs say the shared scene graph exists to prevent.

So the server grows one internal helper that builds a `PriceSeries`, and both
the JSON endpoint and the card call it. The card requests a 30-day window at
`SeriesGroup::World`.

### Frontend changes

`price_history_chart.rs`:

- The `sales: Signal<Vec<SaleHistory>>` prop becomes a `PriceSeries` resource
  keyed on `(item_id, scope, group, hq, selected_range)`.
- `sales_time_domain` derives from `PriceSeries::{from, to}`.
- `TimelineSlicer` currently buckets raw sales for its histogram. It switches to
  summing `units` per bucket from the series, which is both cheaper and correct
  at full history.
- `visible_sales` filtering is replaced by the `from`/`to` request params; the
  slicer becomes a refetch rather than a client-side filter. Debounced, so
  dragging a handle does not issue a request per pointer move.

`item_view.rs` drops the `get_extended_sale_history` effect and the
`extended_sales` / `extended_loading` / `extended_error` signal trio, which
existed to work around the 200-sale SSR payload. The new resource covers both.

## Testing

`ultros-charts` (pure, no I/O — same as existing tests in `charts/`, `data/`):

- `Node::Path` serializes to expected SVG in `svg.rs` tests.
- A `PriceSeries` fixture produces the expected polyline vertex count and axis
  domain; empty series still produce the "No recent sales" card.
- Raw dots are emitted when `raw` is `Some` and omitted when `None`, and the
  emitted dot layer is one `Path` node per series regardless of sale count.

`ultros-clickhouse` (behind `ULTROS_CH_INTEGRATION`, matching the existing
`*_smoke.rs` pattern — throwaway docker ClickHouse plus fixture rows):

- Known fixture sales produce known OHLC. Specifically: `open` is the earliest
  sale's price and `close` the latest within a bucket, verified with sales
  deliberately inserted out of chronological order.
- `gil`/`units` reproduce the VWAP that the current `vwap()` helper computes
  over the same fixture, so the migration provably does not shift prices.
- Bucket boundaries align to absolute UTC time, matching `bucket_seconds`.
- Grouping at datacenter level over two worlds yields one series whose `high` is
  the max of both and whose `p50` differs from either world's `p50` — the
  regression guard for the "quantiles are not re-aggregatable" decision.

`ultros` web layer:

- Scope names at world / datacenter / region all resolve.
- The bucket ladder clamp rejects an absurd `bucket` param.
- The 20,000-bucket cap widens rather than truncates.
- `raw` is present below the threshold and absent above it.

## Risks

- **Cold-cache latency on a popular item at region scope over full history.**
  This is the design's main bet. If p99 is unacceptable, spec 4's structure is
  unaffected and the fix is a `sales_daily` rollup behind the same endpoint
  contract — the client never learns the difference.
- **`FINAL` omission** means an unmerged duplicate can very slightly skew a
  bucket. Judged acceptable at aggregate scale; revisit if the raw table shows
  meaningful duplication in practice.
- **Timeline slicer becoming a refetch** could feel worse than the current
  instant client-side filter. Debounce plus cache should cover it; if not, the
  fallback is to keep the last-fetched wider window client-side and filter
  within it until the drag settles.

## Open questions

- Should outlier filtering become a server-side price predicate (e.g. excluding
  sales outside a MAD band computed in the same query)? Deferred; v1 keeps the
  existing client-side behaviour and only offers the toggle when raw sales are
  present.
- Should the item card PNG path migrate to `PriceSeries` too, so `ultros-charts`
  has exactly one price-history code path? Desirable, but it is a separable
  cleanup and does not block anything.

## Why not a rollup table

A `sales_daily` rollup mirroring `sales_hourly` would follow the established
pattern, and the row count is bounded by actual sale count rather than the
`item × world × day` cross product, so it is feasible. It is deferred because:

- The aggregate is already on the table's primary key prefix; the rollup buys
  speed we may already have.
- It requires a chunked backfill over all history, in the shape of
  `backfill_sales`, which is real work to write and own.
- It introduces a second definition of "what a bucket is" that must stay
  consistent with the on-demand path, or the chart visibly changes shape as the
  user zooms across the tier boundary.

Ship on-demand, measure, promote if the numbers justify it.
