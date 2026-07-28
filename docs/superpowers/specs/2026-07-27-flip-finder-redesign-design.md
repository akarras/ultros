# Flip Finder redesign — spreadsheet shell, derived metrics, saved views

Date: 2026-07-27
Route: `/flip-finder/:world` — `ultros-frontend/ultros-app/src/routes/analyzer.rs`

## Problem

The page buries its content. Measured against live prod (ultros.app, Gilgamesh,
1280x720 viewport), the first data row sits at **y=827** — the entire viewport is
chrome. Breakdown:

| band | px | content |
| --- | --- | --- |
| 0–96 | 96 | site nav |
| 96–184 | 88 | `ToolHeader` — h1 + "About this tool" |
| 216–492 | 276 | controls panel — world picker, 2 toggles, 6 preset buttons, calc `<details>` |
| 524–670 | 146 | filter toolbar — 6 number inputs + 2 buttons |
| 694–746 | 52 | results summary + active-filter chips |
| 770 | — | table container starts |
| 827 | — | first data row |

Two structural faults drive this:

1. **Every filter renders twice** — once as a number input in the toolbar (146px),
   again as a chip in the summary panel (52px). Same state, two representations.
2. **Copy substitutes for UI.** The calc `<details>`, the tips list, and the tool
   header explain what better columns and honest per-row confidence would show
   directly.

The default query compounds it. Sort defaults to ROI, which on Gilgamesh right now
produces this above the fold:

```
Leather Choker   buy 2 gil   profit 213,749,998   ROI 2147483647%
Copper Ring HQ   buy 1 gil   profit  18,999,999   ROI 1900000000%
```

`2147483647` is `i32::MAX`. `analyzer.rs:500` computes
`((profit as f32 / cheapest_price as f32) * 100.0) as i32`, and an f32→i32
saturating cast pins it. `TROLL_MULTIPLE` does not catch these because it guards
inflated *listings*, not inflated *sale history*. So the highest-visibility rows on
the page are laundering artifacts.

## Goals

- Content above the fold; page reads as a spreadsheet.
- Default query serves a seller with ~40 retainer slots: rank by return per slot
  per day, never show dead stock by default.
- Filters directly manipulable; sorting intuitive.
- Filter sets saveable client-side.
- Delete explanatory copy, replacing it with UI that carries the same information.

## Non-goals

- Server-side saved views. localStorage only.
- Fixing the ClickHouse ingest gap (tracked separately — see Finding 1).
- Changing the profit model (`estimated_sale_price`, tax, cross-region).
- Touching the other 7 `VirtualScroller` call sites' behavior.

## Findings that shape the design

### Finding 1 — ClickHouse covers ~7% of traded items

A stratified sample of 150 items drawn from `/api/v1/recentSales/Gilgamesh`
(24,428 items, all with recent sales) returned **11 rows** from
`/api/v1/resale_quality/Gilgamesh` at `window_days=30` — 7.3%. Coverage was flat
across every price band (3–29%, no trend), ruling out a price-correlated launder
filter.

Concrete case — item 44507 (Modern Aesthetics - A Half Times Two), Gilgamesh:

- `extended_history` → 40 sales, 25 within 30 days, newest `2026-07-25T21:16:38`,
  all `world_id: 63`.
- `item_stats` → `{"world_id":63,"item_id":44507,"variants":[]}`.

Downstream causes ruled out: `deep_scan_batch`
(`ultros-clickhouse/src/queries.rs:483`) is a plain key lookup with no sample-size
or confidence filter; `build_refresh_sql`
(`ultros-clickhouse/src/rollups.rs:75`) reads all in-window sales with no LIMIT.
The gap is upstream in the ClickHouse `sales` ingest.

ClickHouse has run in prod since ~May 2026, so >30 days of live ingest have
elapsed — a missing one-shot backfill does not explain an empty 30-day window, and
re-running one would not durably fix it.

**Design consequence:** every column that gates a *default* behavior must be
derivable from data present on 100% of rows. ClickHouse is a refinement layer, not
a dependency. This holds even after the ingest bug is fixed, because CH enrichment
arrives asynchronously per scroll window while the 6-sale buffer is present at
first paint.

### Finding 2 — the 6-sale buffer, characterized

`RecentSales` carries at most 6 sales per `(item, hq)`. Distribution over 24,428
Gilgamesh items:

| sales in buffer | items |
| --- | --- |
| 6 (full) | 21,788 (89.2%) |
| 5 | 508 |
| 4 | 435 |
| 3 | 453 |
| 2 | 559 |
| 1 | 685 |

Observed span across full buffers: **0 hours to 94,041 hours (10.7 years)**. Both
extremes are real and must be guarded — a 0-hour span means six listings bought in
one action, and would divide by zero.

Each sale carries `price_per_unit` and `sale_date` only, newest first.

`compute_summary` already derives `avg_sale_duration` as `(now - oldest) / count`.
Note it measures from *now*, not from the newest sale, so an item whose last 6
sales all happened two years ago yields a correspondingly huge duration. That is
the correct behavior and is preserved.

## Design

### 1. Window-scrolled shell

The page stops being a scrolling document with a table in it, and becomes a control
bar plus a table that owns the rest of the window.

Rather than a fixed-height inner scroll container, the list virtualizes against
**window scroll**. This keeps native scrolling on mobile — no nested scroll trap,
browser chrome auto-hides, momentum scrolling behaves — while filling the viewport
on desktop.

`VirtualScroller` gains a scroll-source mode rather than being forked; the
virtualization math (Fenwick tree, `child_start`, `children_shown`, `translateY`
offset) is identical between modes and should not be duplicated.

```rust
pub enum ScrollSource {
    /// Existing behavior: component owns a fixed-height overflow container.
    Container { viewport_height: f64 },
    /// Page scrolls; list measures against the window.
    /// `sticky_offset` is the height of any sticky chrome above the list, so
    /// rows hidden behind it are not counted as visible.
    Window { sticky_offset: f64 },
}
```

Differences confined to three points:

| | `Container` | `Window` |
| --- | --- | --- |
| scroll position | `div.scroll_top()` | `window.scrollY - list_top` |
| viewport height | `viewport_height` prop | `window.innerHeight - sticky_offset` |
| header stickiness | `sticky top-0` inside container | `sticky top:{sticky_offset}` in page |
| `scroll_to_index` | `div.set_scroll_top` | `window.scrollTo` |

The existing `viewport_height` prop remains and maps to `ScrollSource::Container`,
so all 7 other call sites (`search_box`, `fc_crafting_analyzer`, `leve_analyzer`,
`recipe_analyzer`, `scrip_sources`, `vendor_resale`, `venture_analyzer`) are
untouched.

`visible_range` writeback semantics are unchanged — the lazy CH enrichment effect
in `AnalyzerTable` depends on it and must keep working identically.

**Hydration constraint (load-bearing).** `Container` mode is SSR-safe because
`viewport_height` is a constant: server and client render the same row count.
`Window` mode is not — the server has no `innerHeight`, so a naive implementation
renders N rows server-side and M client-side, producing the tachys
`hydration.rs` mismatch panic this repo has repeatedly hit.

`Window` mode therefore renders a fixed `SSR_FALLBACK_ROWS` count until an
`Effect`-driven `hydrated` flag flips, then switches to the measured height. This
is the established pattern in this codebase — `cheapest_price.rs:48`,
`related_items.rs:131`, `relative_time.rs:32`, `sale_history_table.rs:371`,
`item_explorer.rs:459`. Effects run client-only and after hydration, so the first
client render matches the server's.

Resize and orientation-change listeners update the measured height after
hydration; both must be torn down via `on_cleanup`.

### 2. Derived metrics

Two columns computed from the 6-sale buffer, available on 100% of rows.

**Velocity** — recent sales per day.

```
velocity = count / max(span_days, MIN_SPAN_DAYS)
  where span_days = (now - oldest_sale) in days
```

Equivalent to `86400 / avg_sale_duration.as_secs()`, reusing the existing field.
`MIN_SPAN_DAYS` guards the 0-hour degenerate case. Because the buffer holds the 6
*most recent* sales, this is a genuine estimate of the current rate, not a lower
bound — 600 sales in an hour yields a 6-sale span of ~36 seconds and a
correspondingly high velocity. Resolution degrades at the high end only, which
does not matter for a floor-style filter.

Where ClickHouse has a row, its `sales_per_day` (30-day window, noise-filtered)
supersedes the derived value. The column is populated either way.

**Drift** — is the price falling while I would be holding it.

```
drift_pct = (mean(newest 3) - mean(oldest 3)) / mean(oldest 3) * 100
```

Requires ≥4 sales in the buffer; below that the cell renders `—`. Covers 93.0% of
items (22,731 of 24,428 have 4+ sales). Directly addresses the stated harm: an item whose
price is sliding costs a slot *and* loses value.

This replaces the sparkline `Trend` column as the default price-direction signal.
The sparkline stays available as an opt-in column — it is richer where CH has data,
but at 7% coverage it cannot be the default.

**Confidence** — replaces the removed explanatory copy. Where CH has a row, its
`ConfidenceBand`. Where it does not, a band derived from buffer size and span, so
every row states its own trustworthiness instead of the page disclaiming globally.

### 3. Columns

| column | default | sortable | source |
| --- | --- | --- | --- |
| HQ | required | no | buffer |
| Item | required | name | game data |
| Buy price | required | yes | listings |
| Profit | required | yes | computed |
| Profit / day | required | yes (**default sort**) | computed |
| Velocity | on | yes | derived / CH |
| Drift | on | yes | derived |
| Confidence | on | yes | derived / CH |
| World | on | no | listings |
| Last sold | on | yes | buffer |
| ROI | **off** | yes | computed |
| Sales / day (CH) | off | yes | CH |
| 30d volume | off | yes | CH |
| Datacenter | off | no | listings |
| Sparkline | off | no | CH |

ROI is demoted from required-and-default-sort to an opt-in column. It remains
available — it is a legitimate metric when capital, not slots, is the constraint —
but it is the wrong default for slot scarcity, where absolute return per slot per
day is what matters.

**ROI overflow fix.** Compute in `f64` and clamp to a display ceiling rather than
letting the f32→i32 cast saturate. A row showing `>100,000%` is honest; one showing
`2147483647%` is a bug artifact. The velocity floor removes most such rows from the
default view regardless, but the arithmetic is fixed at the source so the column is
correct whenever a user opts into it.

`ALL_OPTIONAL_COLS` / `DEFAULT_VISIBLE_COLS` / `parse_visible_cols` /
`serialize_visible_cols` keep their existing shape and `?cols=` URL contract. New
column IDs are appended; the "explicit empty set is respected" behavior is retained.

### 4. Default query

Changes from `sort=roi`, no filters, to:

- `sort=profit-per-day`, descending
- velocity ≥ 0.2/day
- ≥2 sales in buffer

Both filters render as **visible, removable chips** in the sticky bar. They are
defaults, not hidden behavior — a user who wants slow high-value movers removes a
chip and sees them.

This is what removes the laundering rows without a special case: a 2-gil item with
a fabricated 213M sale price has no real velocity, so the floor drops it.

Sorting gains ascending/descending. Today `sorted_data` hardcodes `Reverse(...)`;
direction becomes part of the sort state and round-trips through the URL.

### 5. Sticky bar

Two rows, ~76px, replacing 474px of static chrome.

Row 1 — world picker · saved-views menu · row count · Columns · Save view.
Row 2 — active filter chips · `+ Filter`.

Chips *are* the filter UI. Resting state shows `Profit ≥ 100k`; clicking makes the
value an inline input; the `x` clears it. Unset filters live behind `+ Filter`, so
bar height tracks filters in use rather than filters that exist. This deletes the
duplicate-representation problem outright.

Column headers carry sort: click to sort, click again to flip. The active column is
tinted with a direction arrow. This is the spreadsheet-native gesture and reuses
the header row already being stuck.

The bar's height feeds `ScrollSource::Window { sticky_offset }`, and the table
header sticks directly beneath it.

### 6. Saved views

Every filter is already a `query_signal` (`analyzer.rs:429` onward), so the
complete filter state is the URL query string. A saved view is therefore:

```rust
struct SavedView {
    name: String,
    query: String,        // e.g. "?sort=ppd&vel=0.2&profit=100000"
    world: Option<String>, // Some(_) = pinned to that world
}
```

Stored as `Vec<SavedView>` in localStorage via `use_local_storage_with_options::<_,
JsonSerdeCodec>` — the pattern already used by `recently_viewed.rs:39`.

No schema to migrate, and filters added later are captured automatically because
the payload is an opaque string.

**World pinning is opt-in per view.** The world is a path segment
(`/flip-finder/:world`), not a query param, so capturing it is a deliberate choice.
Default is unpinned — a view is a strategy, applied to whatever world is open.
A checkbox at save time pins it, making the view a destination that navigates
worlds on apply. This serves players with characters on different worlds, who want
a dedicated saved view per character.

Sharing is copying the URL; that already works and needs no new surface.

#### Built-in views

The existing preset buttons become the built-in entries in the saved-views menu —
same data shape, not user-deletable. All six move to `last-sold=1d`.

| view | query |
| --- | --- |
| Realistic flips | `?min-buy=5000&last-sold=1d&roi=30&sort=profit-per-day` |
| Big ticket | `?min-buy=100000&last-sold=1d&roi=20&sort=profit` |
| Volume | `?min-buy=1000&last-sold=1d&sort=profit-per-day` |
| 300% return | `?min-buy=1000&last-sold=1d&roi=300&profit=0&sort=profit` |
| 500% return | `?min-buy=10000&last-sold=1d&roi=500&profit=200000` |
| 100k profit | `?min-buy=1000&last-sold=1d&profit=100000` |

Previously these ranged from 3d to 1M. A sale seven days old is weak evidence that
anyone is buying the item today, which is the question a flip actually turns on, so
every view now requires a sale within 24 hours.

Measured against live Gilgamesh data (23,174 rows passing the troll guard), no view
collapses under the tighter window:

| view | at current window | at 1d | at 1d + velocity floor |
| --- | --- | --- | --- |
| Realistic flips | 1,718 (7d) | 324 | 280 |
| Big ticket | 371 (14d) | 77 | 68 |
| Volume | 2,025 (3d) | 701 | 623 |
| 300% return | 1,030 (7d) | 143 | 119 |
| 500% return | 60 (30d) | 9 | 9 |
| 100k profit | 714 (30d) | 91 | 82 |

"500% return" is the thinnest at 9 rows. That is acceptable for a rare-opportunity
scan — the cut removes stale rows, not competitive ones — but it is the view to
re-check if a world with lower liquidity than Gilgamesh reports an empty result.

The two views that set no `sort` inherit the new default (`profit-per-day`) rather
than the old one (`roi`). Views that filter on `roi` keep working with the ROI
column hidden; the filter chip still renders.

### 7. Copy removal

Deleted:

- `ToolHeader` block (88px) — title collapses into the sticky bar.
- Calc formula `<details>` — duplicates `/help/flip-finder`.
- `AssumptionBadge` pair — superseded by the per-row Confidence column.
- Index-page Features grid and Tips list (`Analyzer` component) — three icon cards
  and four bullets restating what the table shows.

Retained:

- Preset buttons, reframed as built-in saved views (see Built-in views above). They
  are the discovery path for new users and cost nothing once views exist as a
  concept.
- `/help/flip-finder`, which is where prose belongs.

### 8. i18n

Per `CLAUDE.md`, every new user-facing string lands in all 7 locale files
(`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) with a real translation, using
`snake_case` keys prefixed `analyzer_`. New keys cover: column headers (velocity,
drift, confidence), confidence band labels, saved-view menu and save dialog, the
pin-to-world checkbox, filter-menu entries, and sort-direction aria labels.

Keys for deleted copy are removed from all 7 files in the same change.

## Testing

Unit-testable without a DOM, in the existing `mod tests` in `analyzer.rs`:

- velocity: full buffer, partial buffer, 0-hour span (`MIN_SPAN_DAYS` guard),
  10-year span
- drift: rising, falling, flat, and the <4-sale `—` fallback
- ROI: no `i32::MAX` saturation for a 1-gil buy against a large profit; clamps to
  the display ceiling
- default query: a synthetic 2-gil launder row is excluded by the velocity floor
- saved views: round-trip serialize/deserialize, pinned and unpinned
- `visible_keys`: existing tests must keep passing unchanged

For `ScrollSource`, the existing pure helpers stay covered; window-mode geometry
needs a browser and is covered by the e2e screenshot harness
(`./scripts/run_e2e.sh`) rather than unit tests.

Note CI does **not** run `cargo test` (commented out in `rust.yml`) — tests must be
run locally before merge. Green CI proves compilation only.

## Risks

| risk | mitigation |
| --- | --- |
| Hydration mismatch from window-measured height | `hydrated` gate; fixed SSR row count. Established pattern in 5+ components here. |
| Default-query change breaks bookmarked URLs | Explicit params in a URL always win over defaults; defaults apply only when absent. |
| Velocity floor hides legitimate slow high-value movers | Floor is a visible, one-click-removable chip, not hidden behavior. |
| Derived velocity disagrees with CH `sales_per_day` | CH wins where present; Confidence column makes the basis visible per row. |
| `ScrollSource` refactor regresses other call sites | Container mode is the default and unchanged; `viewport_height` prop keeps its meaning. |
| localStorage unavailable (private mode) | Views degrade to session-only; the page must not panic. Follows `recently_viewed.rs`. |

## Open items

None blocking. The ClickHouse ingest gap (Finding 1) is real and worth fixing, but
this design deliberately does not depend on it.
