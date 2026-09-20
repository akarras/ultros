# Undercut columns for every analyzer grid (#1377, #1343 WS-J2 part 1)

Two follow-window columns under the shared "Listings" picker group, fed by the
existing windowed listing snapshot: how often sellers drop the price of a
listing, and by how much. Plus the per-window listing slot the rest of the
#1343 history columns will reuse.

## Why

`listing_events` has recorded every board change since 2026-09-07. Sales
already have windowed columns; price movement *between* sales has none. The
alive-set columns from #1417 (`market-alive` = "Active listings", listed units,
sellers, ages) cover "how many listings are up now", so the missing pieces
from #1377 are the two undercut metrics.

Prod evidence (last 7 days, read-only on the box): 810k `updated` events, 96%
of them price drops, median drop 2.6%, mean 13%; plus ~260k remove-then-re-add
price drops per day on the same Universalis listing id. The mean is inflated by
troll listings repriced to sane values, so the median is the column.

## Definitions

An **undercut** is a same-listing price drop observed inside the window
`[from, to)`: the seller lowered the price of a listing Ultros already held.
Two shapes, both counted:

1. `kind = 'updated'` with `prev_price > price_per_unit` (add-first reprice,
   one event).
2. `kind = 'removed'` followed by `kind = 'added'` on the same
   `(item_id, hq, world_id, listing_id)` within 600 s with the add's price below
   the removal's price (remove-first reprice). The add must fall inside the
   window; the removal may precede `from` by up to 600 s, matching the
   reducer's existing `from - 600` read.

Excluded: `source = 'snapshot'` rows (the seed), rows with an empty
`listing_id` (legacy id-less listings), same-price pairs (duplicate publishes
and quantity-only changes), price raises, pairs more than 600 s apart. A
listing repriced twice counts twice.

**Drop** = `(prev_price - price_per_unit) / prev_price`, a fraction in (0, 1].

Scope is additive: a reprice on any world in the scope counts. No cross-world
floor timeline is needed. Floor-based "the scope's floor moved down" metrics
are a separate, later column.

## Wire shape

`ultros-api-types/src/listing_stats.rs`, on `ListingWindowStats`, all
`#[serde(default)]` so stored generations from before the deploy and older
servers still deserialize:

```rust
/// Same-listing price drops observed in the window (both reprice shapes).
pub undercuts: u64,
/// `undercuts` per day of scope-wide listing coverage. `None` only when the
/// scope observed no listing events at all in the window.
pub undercuts_per_day: Option<f64>,
/// Median relative drop across those undercuts, 0..1. `None` when `undercuts == 0`.
pub undercut_median: Option<f64>,
```

`BulkListingStats` is unchanged. The endpoint's synthesized rows for newly
alive keys without a snapshot row keep the defaults (`0`, `None`, `None`),
which the grid renders as `—` for both columns: the rate fold runs inside the
reducer (see below), so a row the endpoint invents never carries one.

## Backend: `ultros-clickhouse/src/listing_history.rs`

### One SQL aggregate per item batch

Inside `window_items`, next to the events and receipts reads, run one more
bounded query with the same `LIMITS`. ClickHouse does the pairing, so the Rust
reducer's per-event memory does not grow (no `listing_id` string is projected
into `WindowEvent`).

```sql
SELECT item_id, hq, count() AS undercuts, quantileExact(0.5)(drop) AS undercut_median
FROM (
  SELECT item_id, hq, (prev_price - price_per_unit) / prev_price AS drop
  FROM (SELECT DISTINCT item_id, hq, world_id, listing_id, event_time, price_per_unit, prev_price
        FROM listing_events
        WHERE kind = 'updated' AND source != 'snapshot'
          AND item_id IN ({items}) AND world_id IN ({worlds})
          AND event_time >= toDateTime({from}) AND event_time < toDateTime({to}))
  WHERE prev_price > price_per_unit
  UNION ALL
  SELECT item_id, hq, (prev_price - price_per_unit) / prev_price AS drop
  FROM (
    SELECT item_id, hq, kind, event_time, price_per_unit,
           lagInFrame(kind)           OVER w AS prev_kind,
           lagInFrame(price_per_unit) OVER w AS prev_price,
           lagInFrame(event_time)     OVER w AS prev_time
    FROM (SELECT DISTINCT item_id, hq, world_id, listing_id, kind, event_time, price_per_unit
          FROM listing_events
          WHERE kind IN ('removed', 'added') AND source != 'snapshot' AND listing_id != ''
            AND item_id IN ({items}) AND world_id IN ({worlds})
            AND event_time >= toDateTime({from} - 600) AND event_time < toDateTime({to}))
    WINDOW w AS (PARTITION BY item_id, hq, world_id, listing_id ORDER BY event_time
                 ROWS BETWEEN 1 PRECEDING AND CURRENT ROW)
  )
  WHERE kind = 'added' AND prev_kind = 'removed'
    AND event_time >= toDateTime({from})
    AND event_time - prev_time <= 600
    AND prev_price > price_per_unit
)
GROUP BY item_id, hq
```

Notes:

- `SELECT DISTINCT` mirrors the events read: an ambiguous insert acknowledgement
  can make the writer retry a batch, and a duplicated `updated` row would
  otherwise count twice.
- The first row of a partition has no predecessor; `lagInFrame` yields the
  type default, and `prev_kind = 'removed'` rejects it.
- Result row type: `RepriceRow { item_id: i32, hq: u8, undercuts: u64, undercut_median: f64 }`.
  Merged into the batch's `output` by `(item_id, hq != 0)`. A key present in
  `RepriceRow` is always already present in `output` (its add/update event is
  in the same read), but merge defensively with `entry().or_insert_with(default window)`.
  `undercut_median = Some(row.undercut_median)` only when `undercuts > 0`.

### Rate per day, computed once per generation

In `window()` after all batches merge: scope coverage is the fold over every
key's `listing_coverage` — `first = min(first_observed_unix)`,
`last = max(last_observed_unix)`,
`span_secs = clamp(last - max(first, from), 86400, days * 86400)`.
Then for every key `undercuts_per_day = Some(undercuts as f64 / (span_secs as f64 / 86400.0))`.
If no key has any observation, leave `None` everywhere.

Why scope-wide and not per item: an item's own span measures its activity, not
ingestion coverage. Two undercuts three hours apart would read as 16/day. The
scope span is the honest denominator, and it makes the 30/90-day windows read
correctly while they fill (history starts 2026-09-07).

The one-day floor keeps a scope with a few minutes of data from reporting
absurd rates; the window cap keeps floating-point drift from exceeding the
window.

### Not changed

`match_observations` and `MatchedSalesStats.repriced` stay as they are: they
serve the sales matcher, not this column. The snapshot publisher, manifest,
cadence and limits are untouched; the extra query is bounded per batch like
the others.

## Frontend

### API client (`ultros-frontend-core/src/api.rs`)

`get_listing_stats_window(scope_name, days: u16)` →
`/api/v1/listing_stats/{scope}?window={days}`. The existing window-free
`get_listing_stats` stays.

### Per-window listing slots (`analyzer_kit/market.rs`)

`MarketData` gains, beside the window-free `listings` slot:

```rust
listing_windows: [RwSignal<ScopedListings>; Window::ALL.len()],
listing_windows_wanted: [RwSignal<bool>; Window::ALL.len()],
```

- `listing_window(self, window) -> Option<ListingSlot>` with the same scope
  check as `listings()`.
- `want_listing_window(self, window)`: idempotent, never un-wants.
- `fetch_listing_stats` takes `days: Option<u16>`; `None` is today's body.
  Scope-change and generation guards are identical.
- **Retry while the server builds the snapshot.** The first request for a
  scope and window returns 503 until the background worker publishes a
  generation (about a minute for a world, longer for a DC). A failed windowed
  fetch keeps the slot `None` (cells show the pending label) and retries after
  15 s, 30 s and 60 s. After the fourth failed attempt (three retries) the
  slot is `Some` with `failed = true`. The window-free body keeps its
  no-retry behaviour.
- The `needs` effect: `if listing_window_wanted(n) { market.want_listing_window(selected) }`
  where `listing_window_wanted(needs) -> bool` is true when any
  `LISTING_WINDOW_COLUMNS` id is in the wanted set (visible column, `?cols=`,
  filter, sort target). Only the selected window is ever fetched; there are no
  pinned `-N` variants because every window body is a separate multi-megabyte
  fetch. Changing the page window wants the new window's slot the same way.
- `sizing_version` / `track_all` include the new slots so auto-fit re-measures
  when a body lands.

### Columns (`analyzer_kit/stat_columns.rs`)

```rust
pub enum ListingWindowKind { UndercutsPerDay, UndercutMedian }
pub static LISTING_WINDOW_COLUMNS: [(ListingWindowKind, &str); 2] = [
    (ListingWindowKind::UndercutsPerDay, "market-undercuts"),
    (ListingWindowKind::UndercutMedian, "market-undercut-pct"),
];
```

- Labels follow the window like the follow-window sale columns:
  `with_window(name, selected)` → "Undercuts/day (7d)", "Undercut % (7d)".
- Picker group: the existing "Listings" heading, listed after the five alive
  columns. `shared_cols_in`, `market_picker_options`, `market_metrics()` and
  the id-uniqueness test include them. Nothing is default-on.
- Hints (`title`): undercuts/day = "Same-listing price drops per day across
  the scope, both edit-in-place and remove-and-relist. Raises and new listings
  are not counted." Undercut % = "Median drop as a share of the previous price
  across those undercuts."

### Values (`MarketMetric::ListingWindow(kind)`)

| State | Value |
|---|---|
| slot `None` | `Pending` |
| slot `failed` | `Unavailable` |
| row absent, or `window` is `None` | `Missing` (`—`) |
| `UndercutsPerDay` | `Number(undercuts_per_day)`; `None` → `Missing` |
| `UndercutMedian` | `Number(undercut_median * 100)`; `None` → `Missing` |

Display: undercuts/day `{:.2}` (same branch as sales/day); undercut %
`{:.1}%`. Both are `Number`s, so header sort and the metric filter registry
work without new code; the filter menu lists them under "Listings" like the
alive columns.

### Locale keys

Added to all seven files in `ultros-frontend/ultros-i18n/locales/`
(`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`), real translations:
`market_undercuts`, `market_undercut_pct`, `market_undercuts_title`,
`market_undercut_pct_title`.

## Tests

- **Wire:** `old_wire_shape_still_deserializes` gains the three fields;
  round-trip covers `Some`/`None`.
- **ClickHouse (gated, `ULTROS_CH_INTEGRATION=1`, throwaway container):** a
  case in `listing_history_smoke.rs` seeding one item with: an `updated`
  100→90; a removed@100 then added@80 on the same `listing_id` 30 s later; a
  removed@100 then added@120 (raise); a pair 20 min apart; an `updated` with
  `source = 'snapshot'`; a pair with empty `listing_id`; a duplicated `updated`
  row. Expect `undercuts = 2`, `undercut_median = 0.15`, and
  `undercuts_per_day` = 2 / clamp(scope span). A second item with no reprices
  expects `0`, `Some(0.0)`, `None`.
- **Rate fold:** a pure unit test for the scope-coverage clamp (no coverage →
  `None`; five-minute span → one-day floor; 40-day span in a 30-day window →
  30-day cap).
- **Frontend unit:** `stat_columns` tests for ids, labels with window suffix,
  picker group and `listing_window_wanted`; `market.rs` value test for the
  five states above.
- **E2E:** `integration/market-window.cjs` already fixtures
  `listing_stats/Cactuar?window=1`; extend the fixture with the new fields and
  assert: cells render `0.29` and `2.6%`, headings switch `(7d)`→`(30d)` with
  the window select, the `?window=` request fires only once a column is
  wanted, and a `?cols=market-undercuts` deep link with `sort=grid:market-undercuts`
  sorts. `shared-analyzer-data.cjs`'s shared-id list gains both ids so every
  analyzer host is probed.
- `./check_ci.sh` green before the PR.

## Out of scope

- The item page (#1377 mentions it; separate change).
- Floor-drop metrics from `floor_changes`.
- Pinned per-window variants (`market-undercuts-30`).
- The other #1343 WS-J2 columns (floor min/max, additions/removals,
  time-to-sell, days of stock, the floor-30 sparkline). Each becomes a
  `ListingWindowKind` variant on the slot this change adds.
- Slimming the windowed payload (17 MB for one world with 7 days of history).
  The fetch is lazy, but this is the next cost to look at once the columns are
  in use.

## Risks

- **First-request latency.** A cold scope/window pair 503s until the worker
  publishes; the retry ladder covers roughly two minutes. A DC scope that
  takes longer shows `—` (Unavailable) until the user reloads.
- **Payload size.** One windowed body per scope per selected window; changing
  the page window fetches again. Acceptable for an opt-in column.
- **Partial history in long windows.** The rate uses scope coverage so it
  reads correctly; the raw count is not shown.
