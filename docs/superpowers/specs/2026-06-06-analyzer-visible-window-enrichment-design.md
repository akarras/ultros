# Analyzer visible-window lazy enrichment — design

- **Date:** 2026-06-06
- **Status:** Approved (ready for implementation plan)
- **Area:** `ultros-frontend/ultros-app` (Flip Finder / analyzer)

## Problem

The analyzer table shows Trend (sparkline), Sales/day, and 30d Volume columns,
but only the **top ~240 rows by ROI** ever get populated — everything below the
cap renders `—`. On a busy world the table holds **thousands** of rows, so most
of them are blank. The user wants every row the analyzer *shows them* to carry
its enrichment, fetched efficiently.

## Current behavior (baseline)

The blanks are by design, not an N+1 query bug:

- After building the in-memory profit table, the frontend enriches only the top
  `ENRICHMENT_BATCH_CAP = 240` rows by ROI —
  `select_enrichment_keys(profits, 240)` at `analyzer.rs:392`.
- It fires **two batch calls** (`analyzer.rs:1479`, inside a `LocalResource` in
  the parent `AnalyzerWorldView`):
  - `get_resale_quality(world, top240, 30)` → Sales/day + 30d Volume +
    confidence/launder flags, from ClickHouse `item_stats_window`.
  - `post_sparklines(world, { items: top200, hours: 168 })` → Trend, from
    `sales_hourly`. Sparklines uses a tighter `SPARKLINE_BATCH_CAP = 200`.
- Rows below the cap render `—`. The caps exist only because the backend rejects
  payloads above 250 / 200 items (`resale_quality.rs`, `movers.rs`).

Two facts that make the fix tractable:

1. **The backend already does true bulk queries** —
   `WHERE (item_id,hq,world_id) IN (...)`, one ClickHouse query per batch, no
   per-item loop (`ultros-clickhouse/src/queries.rs` `deep_scan_batch`,
   `sparklines_batch`). The limit is purely the frontend cap + per-request size
   ceiling.
2. **Row order is fully determined by Pass-1 data.** The only sort modes are
   `Roi` / `Profit` / `ProfitPerDay` (`analyzer.rs:617`), all computed from the
   in-memory profit table. Nothing sorts or filters on enriched columns *except*
   the "hide suspicious" filter (`analyzer.rs:599`). So the set of visible rows
   and their order are known *before* enrichment — which is what makes
   visible-window fetching viable.

The table is virtualized: `viewport_height=720`, `row_height=40`, `overscan=8`
→ ~26 rows mounted at once (`analyzer.rs:1051`). `sorted_data` collects **all**
filtered rows with no `.take()` (`analyzer.rs:492`); the `VirtualScroller` just
virtualizes them.

## Goal

Fetch Trend + Sales/day + 30d Volume (+ suspicious flag) for the rows currently
in or near the viewport, accumulating results into one map as the user scrolls.
Each scroll-stop fetches ~50–90 keys (one batch, under the 200/250 ceilings),
scales to any world size, and never re-fetches a row it already holds.

## Non-goals

- No backend, API-type, or ClickHouse query changes (existing bulk endpoints
  suffice).
- No new sort/filter on enriched columns (visible-window deliberately does not
  hold the full set in memory, so full-set sort/filter is not enabled here).
- No locale changes — no new user-facing strings are introduced.
- The prod `item_stats_window` backfill (~7% coverage) is **out of scope**: rows
  ClickHouse has no rollup for stay blank regardless of fetch strategy. Trends
  from `sales_hourly` are unaffected. Flagged separately as a data-population
  action.

## Design

### 1. `VirtualScroller` — expose its visible range (additive, optional)

File: `ultros-frontend/ultros-app/src/components/virtual_scroller.rs`

Add one optional prop so the other 7 consumers are untouched:

```rust
#[prop(optional, into)] visible_range: Option<RwSignal<(usize, usize)>>,
```

When provided, an `Effect` writes `(start, end)` derived from the existing
`child_start` / `children_shown` memos whenever they change:

```rust
if let Some(vr) = visible_range {
    Effect::new(move |_| {
        let start = (child_start() as usize).min(children_len().saturating_sub(1));
        let end = (start + children_shown() as usize).min(children_len());
        vr.set((start, end));
    });
}
```

No prop → no effect → zero behavior change for `venture_analyzer`,
`recipe_analyzer`, `leve_analyzer`, `fc_crafting_analyzer`, `scrip_sources`,
`vendor_resale`, `search_box`.

The exposed range is the *rendered* range (includes the scroller's overscan).
Any extra prefetch margin is applied consumer-side (keeps the scroller honest
about what it actually renders).

### 2. `AnalyzerTable` — own enrichment + orchestrate lazy fetch

File: `ultros-frontend/ultros-app/src/routes/analyzer.rs`

The enrichment fetch moves **out of the parent** (`AnalyzerWorldView`) and into
`AnalyzerTable`, which already owns `sorted_data` and renders the
`VirtualScroller`. The `enrichment` prop on `AnalyzerTable` is removed; the
parent's `LocalResource` (currently `analyzer.rs:1441-1517`) and the
`select_enrichment_keys` / `ENRICHMENT_BATCH_CAP` / `SPARKLINE_BATCH_CAP` machinery
are deleted.

State held in `AnalyzerTable`:

```rust
let enrichment = RwSignal::new(EnrichmentMaps::default()); // accumulates, never replaced
let requested  = StoredValue::new(HashSet::<(i32, bool)>::new()); // dedupe + loop-breaker
let visible_range = RwSignal::new((0usize, 0usize));
```

Wiring:

- Pass `visible_range=visible_range` into `<VirtualScroller .../>`.
- Debounce the range with leptos-use (`signal_debounced`, ~150 ms) so a fast
  fling only fetches where scrolling settles. (`leptos-use` is already a
  workspace dep; `gloo_timers::future::TimeoutFuture` — as used in
  `search_box.rs` — is the fallback if a manual debounce reads cleaner.)
- A `pending_keys` memo computes the keys to fetch:

```rust
const PREFETCH_MARGIN: usize = 30; // rows above & below the rendered window

fn visible_keys(
    data: &[(usize, CalculatedProfitData)],
    range: (usize, usize),
    margin: usize,
    seen: &HashSet<(i32, bool)>,
) -> Vec<(i32, bool)> {
    let (start, end) = range;
    let lo = start.saturating_sub(margin);
    let hi = (end + margin).min(data.len());
    data.get(lo..hi)
        .unwrap_or(&[])
        .iter()
        .map(|(_, d)| (d.inner.sale_summary.item_id, d.inner.sale_summary.hq))
        .filter(|k| !seen.contains(k))
        .collect()
}
```

`visible_keys` is a **pure free function** so it is unit-testable without a DOM.
The `pending_keys` memo reads the debounced range + `sorted_data` + `requested`
and calls it.

- An `Effect` reacts to `pending_keys`: if non-empty, mark them requested
  (optimistic dedupe), then `spawn_local` a fetch that joins both batch calls and
  merges into `enrichment`:

```rust
Effect::new(move |_| {
    let keys = pending_keys.get();
    if keys.is_empty() { return; }
    let world_name = world.get_untracked();
    requested.update_value(|s| s.extend(keys.iter().copied()));
    spawn_local(async move {
        // chunk to <=200 (sparklines) / <=240 (quality); usually 1 chunk
        let (quality, sparklines) = futures::join!(
            get_resale_quality(&world_name, keys.clone(), 30),
            post_sparklines(&world_name, SparklinesRequest { items: keys.clone(), hours: Some(168) }),
        );
        // Merge whatever succeeded into the accumulating map (mirrors the
        // existing merge at analyzer.rs:1492-1509).
        enrichment.update(|m| {
            if let Ok(q) = &quality {
                m.quality.extend(q.rows.iter().map(|r| ((r.item_id, r.hq), r.clone())));
            }
            if let Ok(s) = &sparklines {
                m.sparkline.extend(s.series.iter().map(|r| ((r.item_id, r.hq), r.points.clone())));
            }
        });
        // If a call failed, drop its keys from `requested` so revisiting retries.
        if quality.is_err() || sparklines.is_err() {
            requested.update_value(|s| keys.iter().for_each(|k| { s.remove(k); }));
        }
    });
});
```

Defensive chunking keeps each request under the caps even with a tall viewport +
margin; in practice ~26 + 2×30 ≈ 86 keys → one chunk.

Fixed params preserved from today: `window_days = 30`, sparkline `hours = 168`.

### 3. Cell rendering — loading vs. no-data

Files/lines: Trend `analyzer.rs:1319`, Sales/day `analyzer.rs:1340`,
30d Volume `analyzer.rs:1349`.

Today every cell falls back to empty/`—` whether the row is loading or genuinely
absent. Add an in-flight notion (a key is in `requested` but not yet in
`enrichment`): render the already-imported `BoxSkeleton` for Trend and a dim
placeholder for the number columns while in-flight, and `—` only once a fetch has
completed with no data. This keeps fast scrolling reading as "loading", not
"no data".

### 4. Caching / invalidation

- Reset `enrichment` + `requested` when **`world`** changes (enrichment is
  world-specific; the underlying data resources reload anyway). An `Effect`
  watching `world` clears both.
- Sort/filter changes do **not** invalidate: switching ROI→Profit reuses
  everything already fetched; only newly-visible items fetch. The fixed
  window/hours mean no per-window cache key is needed.

## Data flow

```
scroll → VirtualScroller updates child_start/children_shown
       → visible_range RwSignal set (rendered range)
       → signal_debounced(150ms)
       → pending_keys memo = visible_keys(sorted_data, range, MARGIN, requested)
       → Effect: mark requested, spawn_local fetch (join resale_quality + sparklines)
       → enrichment.update(merge)
       → cells re-read enrichment, render values (mounted rows only)
```

## Edge cases & wrinkles (handled)

- **Suspicious-filter reflow** (`analyzer.rs:599-614`): a row kept while
  unenriched can disappear once its data arrives and flags it
  Unusable/high-launder, shifting rows. The prefetch margin means this mostly
  happens off-screen (the row is enriched before it scrolls into view).
  Decision: keep current **hide** semantics; switching to dim/mark would fully
  remove reflow but is out of scope.
- **Reactive loop** (enrichment update → `sorted_data` recompute, because the
  suspicious filter reads enrichment → `pending_keys` recompute): the `requested`
  set breaks it — already-requested keys are filtered out, so it converges after
  one round per newly-visible set.
- **Fetch failure:** failed keys are cleared from `requested` so revisiting the
  rows retries, avoiding a permanent `—`.
- **Initial mount:** the scroller's range effect runs on mount, setting
  `visible_range` to `(0, ~26)`, which triggers the first fetch of the default
  (top-ROI) view — so the default view fills without a separate eager path.
  `AnalyzerTable` receives already-resolved data (it builds `ProfitTable::new`
  synchronously at `analyzer.rs:429`), so keys are derivable at mount.

## Side benefits

- **Fixes a latent mismatch:** the current eager pass always ranks by ROI even
  when the user sorts by Profit / Profit-per-day, enriching the wrong rows.
  Reading `sorted_data` makes enrichment always match what is shown.
- Lighter first paint (~50–90 keys vs. 240 + 200).
- No row is permanently blank merely for being far down the list.

## Testing

- **Unit test** for `visible_keys` (range + margin clamping, dedupe vs. `seen`,
  empty-data and out-of-bounds ranges) — pure function, no DOM.
- `./check_ci.sh` — `cargo fmt --all -- --check` + `cargo clippy --all-targets
  -- -D warnings` (CI gate per CLAUDE.md). Requires submodule init; if blocked,
  at minimum run fmt-check and note clippy was skipped.
- **Manual browser smoke** (per local-browser-testing notes): load the analyzer
  on a busy world, scroll, and confirm (a) rows fill in progressively, (b) the
  network panel shows small *batched* requests per scroll-stop (not per-row),
  (c) already-seen rows do not refetch when scrolled back to, (d) the loading
  shimmer shows for in-flight rows.

## Files touched

- `ultros-frontend/ultros-app/src/components/virtual_scroller.rs` — add optional
  `visible_range` writeback prop + effect.
- `ultros-frontend/ultros-app/src/routes/analyzer.rs` — move enrichment into
  `AnalyzerTable`; add visible-window orchestration (debounced range →
  `pending_keys` → fetch → accumulate); loading state in the three cells; remove
  the eager top-N path (`select_enrichment_keys`, the two `*_BATCH_CAP` consts,
  the parent `LocalResource`, the `enrichment` prop); extract testable
  `visible_keys` helper.

## Out of scope (flagged)

- Prod `item_stats_window` backfill (`clickhouse_backfill` one-shot bin never
  run; ~7% item coverage). Independent of fetch strategy; run to densify
  Sales/day + 30d Volume coverage.
- Enabling full-set sort/filter/export by enriched columns (would require holding
  all rows' data in memory — the "fetch all up-front" alternative we did not
  choose).
- Switching the suspicious filter from hide to dim/mark.
