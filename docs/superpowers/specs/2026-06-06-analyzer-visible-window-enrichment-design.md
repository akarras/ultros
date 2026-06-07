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

`EnrichmentMaps` (defined at `analyzer.rs:34`) gains a third field so cells can
tell "still loading" from "fetched, no data":

```rust
#[derive(Clone, Debug, Default)]
struct EnrichmentMaps {
    quality: HashMap<(i32, bool), ResaleQualityRow>,
    sparkline: HashMap<(i32, bool), Vec<u32>>,
    settled: std::collections::HashSet<(i32, bool)>, // keys whose fetch completed (data or not)
}
```

State held in `AnalyzerTable`:

```rust
// Accumulating enrichment (quality + sparkline + settled); grown, never replaced.
let enrichment = RwSignal::new(EnrichmentMaps::default());
// Scheduled-set: keys we've already kicked a fetch for. Non-reactive
// (StoredValue) — it's the dedupe + reactive-loop breaker, not UI state.
let requested = StoredValue::new(std::collections::HashSet::<(i32, bool)>::new());
let visible_range = RwSignal::new((0usize, 0usize));
```

Wiring:

- Pass `visible_range=visible_range` into `<VirtualScroller .../>`.
- The key selection is a **pure free function** (unit-testable without a DOM):

```rust
const PREFETCH_MARGIN: usize = 30; // rows above & below the rendered window

/// Keys in the [start-margin, end+margin) slice of `data`, minus `seen`.
/// Generic over the row type + key extractor so it unit-tests with plain
/// `(i32, bool)` fixtures — no `CalculatedProfitData` / DOM needed.
fn visible_keys<T>(
    data: &[T],
    range: (usize, usize),
    margin: usize,
    seen: &std::collections::HashSet<(i32, bool)>,
    key_of: impl Fn(&T) -> (i32, bool),
) -> Vec<(i32, bool)> {
    let (start, end) = range;
    let lo = start.saturating_sub(margin);
    let hi = (end + margin).min(data.len());
    data.get(lo..hi)
        .unwrap_or(&[])
        .iter()
        .map(key_of)
        .filter(|k| !seen.contains(k))
        .collect()
}
```

- One `Effect` does selection + debounce + fetch + merge. It reads
  `visible_range` and `sorted_data` reactively, and `requested` non-reactively
  (so claiming a key doesn't retrigger the effect). Debounce uses a generation
  guard + `gloo_timers::future::TimeoutFuture` — the same pattern `search_box.rs`
  already uses — so a fast fling only fetches where it settles. (Chosen over
  leptos-use's `signal_debounced`: the gloo-timer pattern is already proven here
  and adds no new API surface.) Keys are claimed in `requested` *after* the
  debounce, so superseded generations never claim.

```rust
const DEBOUNCE_MS: u32 = 150;
// Generation counter for debounce-with-cancellation, mirroring the proven
// pattern in components/search_box.rs. (`gen` is a reserved keyword in Rust
// edition 2024, so this is named `fetch_id`.)
let fetch_id = RwSignal::new(0u64);

Effect::new(move |_| {
    let range = visible_range.get();          // reactive: scroll
    let keys = sorted_data.with(|data| {      // reactive: sort / filter / enrichment
        requested.with_value(|seen| {
            visible_keys(data, range, PREFETCH_MARGIN, seen, |(_, d)| {
                (d.inner.sale_summary.item_id, d.inner.sale_summary.hq)
            })
        })
    });
    if keys.is_empty() {
        return;
    }
    fetch_id.update(|n| *n += 1);
    let current_id = fetch_id.get_untracked();
    let world_name = world.get_untracked();
    leptos::task::spawn_local(async move {
        TimeoutFuture::new(DEBOUNCE_MS).await;       // debounce
        if fetch_id.get_untracked() != current_id {
            return; // superseded by a newer range
        }
        requested.update_value(|s| s.extend(keys.iter().copied())); // claim post-debounce
        // window <= ~86 keys << 200 cap -> single batch (see PREFETCH_MARGIN note)
        let (quality, sparklines) = futures::join!(
            get_resale_quality(&world_name, keys.clone(), 30),
            post_sparklines(
                &world_name,
                SparklinesRequest { items: keys.clone(), hours: Some(168) },
            ),
        );
        // Merge whatever succeeded, and mark every fetched key `settled`
        // (success OR error) so cells switch loading -> value / "—".
        // Mirrors the existing merge at analyzer.rs:1492-1509.
        enrichment.update(|m| {
            if let Ok(q) = &quality {
                m.quality
                    .extend(q.rows.iter().map(|r| ((r.item_id, r.hq), r.clone())));
            }
            if let Ok(s) = &sparklines {
                m.sparkline
                    .extend(s.series.iter().map(|r| ((r.item_id, r.hq), r.points.clone())));
            }
            m.settled.extend(keys.iter().copied());
        });
        // No per-key retry: keys stay in `requested`, so a CH blip degrades the
        // rows to "—" (same as today) rather than refetch-looping. A world
        // change resets everything (see §4).
    });
});
```

The window is ~26 + 2×30 ≈ 86 keys — comfortably under the 200 / 250 caps — so
keys are sent in a single batch (no chunking). If `PREFETCH_MARGIN` or the
viewport ever grows enough to approach ~150 keys, add chunking to ≤200 / ≤240.

Fixed params preserved from today: `window_days = 30`, sparkline `hours = 168`.

### 3. Cell rendering — loading vs. no-data

Files/lines: Trend `analyzer.rs:1319`, Sales/day `analyzer.rs:1340`,
30d Volume `analyzer.rs:1349`.

Today every cell falls back to empty/`—` whether the row is loading or genuinely
absent. With the `settled` set, each of the three cells follows one rule (reading
the already-subscribed `enrichment` signal):

- key in `quality` / `sparkline` map → render the value (current behavior).
- else if key in `settled` → render `—` (fetched, no CH data).
- else → render `SingleLineSkeleton` (a thin themed pulse, `skeleton.rs:6`) — the
  fetch is in flight.

`SingleLineSkeleton` (not `BoxSkeleton`, which is a six-row block sized for a
panel) is added to the existing `skeleton::{...}` import. This keeps fast
scrolling reading as "loading", not "no data".

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
       → Effect: keys = visible_keys(sorted_data, range, MARGIN, requested)
       → debounce (gen guard + TimeoutFuture 150ms); latest generation wins
       → claim keys in `requested`; join get_resale_quality + post_sparklines
       → enrichment.update(merge data + mark keys settled)
       → cells re-read enrichment → value / "—" / skeleton (mounted rows only)
```

## Edge cases & wrinkles (handled)

- **Suspicious-filter reflow** (`analyzer.rs:599-614`): a row kept while
  unenriched can disappear once its data arrives and flags it
  Unusable/high-launder, shifting rows. The prefetch margin means this mostly
  happens off-screen (the row is enriched before it scrolls into view).
  Decision: keep current **hide** semantics; switching to dim/mark would fully
  remove reflow but is out of scope.
- **Reactive loop** (enrichment update → `sorted_data` recompute, because the
  suspicious filter reads enrichment → the fetch effect re-runs): the `requested`
  set breaks it — already-claimed keys are filtered out by `visible_keys`, so the
  recomputed key set is empty and the effect returns. Converges after one round
  per newly-visible set.
- **Fetch failure:** on completion (success *or* error) the fetched keys are
  marked `settled` and kept in `requested`, so the rows render `—` (graceful
  degradation, same as today's CH-outage behavior) with no refetch loop. A world
  change clears `settled` + `requested`, allowing a fresh attempt.
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
