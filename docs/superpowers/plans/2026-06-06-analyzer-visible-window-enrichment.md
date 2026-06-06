# Analyzer Visible-Window Lazy Enrichment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the analyzer's fixed top-240/200 ROI enrichment cap with viewport-driven lazy fetching, so every row the user scrolls to gets its Trend / Sales-day / 30d-Volume, fetched in small batched requests that accumulate.

**Architecture:** `VirtualScroller` gains an optional `visible_range` writeback signal. `AnalyzerTable` owns an accumulating `EnrichmentMaps` and a `requested` dedupe set; one debounced effect (generation-guard + `gloo` `TimeoutFuture`, mirroring `components/search_box.rs`) selects the visible-window keys from `sorted_data`, fetches them via the existing bulk endpoints, and merges results. Cells distinguish loading / no-data / value via a new `settled` set. The old eager fetch in the parent (`AnalyzerWorldView`) is deleted.

**Tech Stack:** Rust, Leptos (reactive signals/effects/memos), `leptos` edition 2024, `gloo-timers` (futures), ClickHouse-backed `resale_quality` + `sparklines` HTTP endpoints (unchanged).

**Design spec:** `docs/superpowers/specs/2026-06-06-analyzer-visible-window-enrichment-design.md`

---

## Prerequisites (build/test environment)

Building or testing `ultros-app` compiles the whole workspace under the default `ssr` feature, which needs the game-data submodule and (on Windows) a vendored-OpenSSL toolchain. Before running any `cargo`/`./check_ci.sh` command:

- **Submodule:** `git submodule update --init --recursive --depth=1` (the `xiv-gen-db` build script reads `xiv-gen/ffxiv-datamining/`; non-recursive init is insufficient — `cn/ko/tc` CSVs live in nested submodules).
- **Windows OpenSSL (vendored):** put Strawberry Perl + its C toolchain ahead of Git's MSYS Perl on PATH, e.g. from PowerShell:
  `\$env:PATH = "C:\Strawberry\perl\bin;C:\Strawberry\c\bin;" + \$env:PATH` (Git Bash: prepend `/c/Strawberry/perl/bin:/c/Strawberry/c/bin:`).
- **Worktree builds:** this repo is checked out as a git worktree under `.claude/worktrees/`. Point cargo at the main checkout's warm target dir to avoid a cold ~10-min rebuild: `export CARGO_TARGET_DIR=<main-repo>/target` (see AGENTS.md / project notes).

CI's gate is `./check_ci.sh` = `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`. It does **not** run `cargo test`, but `--all-targets` compiles the test module, so tests must compile. Run `cargo test -p ultros-app` yourself to actually execute the unit tests added here.

No new user-facing strings are introduced, so **no locale files change**.

---

## File Structure

- `ultros-frontend/ultros-app/src/components/virtual_scroller.rs` — **modify.** Add one optional `visible_range: Option<RwSignal<(usize, usize)>>` prop and an effect that writes the rendered `(start, end)` when the prop is supplied. Purely additive; the 7 other call sites pass nothing and are unaffected.
- `ultros-frontend/ultros-app/src/routes/analyzer.rs` — **modify.** (1) Extend `EnrichmentMaps` with a `settled` set + `is_settled`. (2) Add a pure, generic `visible_keys` helper + unit tests. (3) Move enrichment ownership into `AnalyzerTable`: accumulating signal, dedupe set, visible-range wiring, world-reset effect, debounced fetch effect. (4) Update the three enrichment cells for loading/no-data/value. (5) Delete the parent's eager fetch (`LocalResource`, `enrichment_signal`, the `enrichment` prop) and the now-dead `roi_for_rank`, `select_enrichment_keys`, `ENRICHMENT_BATCH_CAP`, `SPARKLINE_BATCH_CAP`.

No other files change.

---

## Task 1: VirtualScroller — optional `visible_range` writeback prop

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/virtual_scroller.rs` (prop list ~line 56-68; after the `children_shown` memo ~line 148)

This is a structural component change with no natural unit test; it is verified by compilation here and by the manual scroll check in Task 3. The other 7 `<VirtualScroller>` consumers do not pass the new optional prop, so they are unaffected.

- [ ] **Step 1: Add the optional prop to the component signature**

In the `#[component] pub fn VirtualScroller<...>(...)` argument list, add the new prop immediately after the existing `scroller_ref` prop:

```rust
    #[prop(optional)] scroller_ref: Option<NodeRef<leptos::html::Div>>,
    /// Optional writeback of the rendered row range `(start, end)` (end
    /// exclusive, includes overscan). Lets a parent fetch data only for
    /// rows in view. When omitted, no extra work is done.
    #[prop(optional, into)] visible_range: Option<RwSignal<(usize, usize)>>,
) -> impl IntoView
```

- [ ] **Step 2: Write the range back whenever the rendered window changes**

Right after the `children_shown` memo is defined (the block ending with `((effective_viewport / avg_row_height()).ceil() as u32).max(1) + render_ahead`), add:

```rust
    // Publish the rendered row range to an optional parent signal. `child_start`
    // and `children_shown` already account for overscan and match the slice used
    // by `virtual_children` below.
    if let Some(range_sig) = visible_range {
        Effect::new(move |_| {
            let len = children_len();
            if len == 0 {
                range_sig.set((0, 0));
            } else {
                let start = (child_start() as usize).min(len - 1);
                let end = (start + children_shown() as usize).min(len);
                range_sig.set((start, end));
            }
        });
    }
```

- [ ] **Step 3: Verify the crate still compiles (no consumer broke)**

Run: `cargo check -p ultros-app`
Expected: compiles with no errors. (The new prop is optional; existing `<VirtualScroller>` uses in `analyzer.rs`, `venture_analyzer.rs`, `recipe_analyzer.rs`, `leve_analyzer.rs`, `fc_crafting_analyzer.rs`, `scrip_sources.rs`, `vendor_resale.rs`, `search_box.rs` compile unchanged.)

- [ ] **Step 4: Run the format + lint gate**

Run: `./check_ci.sh`
Expected: passes (fmt clean, clippy clean).

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/virtual_scroller.rs
git commit -m "feat(virtual-scroller): optional visible_range writeback prop

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `visible_keys` pure helper + unit tests (TDD)

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (add helper after `select_enrichment_keys` ~line 400; add tests inside the existing `#[cfg(test)] mod tests` ~line 1872)

The helper is generic over the row type and a key extractor so it tests with plain `(i32, bool)` tuples — no `CalculatedProfitData` fixtures, no DOM. It is wired into the real fetch effect in Task 3 (which removes the temporary `#[allow(dead_code)]`).

- [ ] **Step 1: Write the failing tests**

Add these tests to the **existing** `mod tests` block in `analyzer.rs` (it already starts with `use super::*;`). Append them after the last existing test:

```rust
    #[test]
    fn visible_keys_includes_window_and_margin() {
        let data: Vec<(i32, bool)> = (0..100).map(|i| (i, false)).collect();
        let seen = std::collections::HashSet::new();
        // rendered rows [40, 50), margin 5 => slice [35, 55)
        let keys = visible_keys(&data, (40, 50), 5, &seen, |k| *k);
        assert_eq!(keys.len(), 20);
        assert_eq!(keys.first(), Some(&(35, false)));
        assert_eq!(keys.last(), Some(&(54, false)));
    }

    #[test]
    fn visible_keys_clamps_at_start_and_end() {
        let data: Vec<(i32, bool)> = (0..10).map(|i| (i, false)).collect();
        let seen = std::collections::HashSet::new();
        // lo = 2.saturating_sub(5) = 0 ; hi = (4 + 5).min(10) = 9 => slice [0, 9)
        let keys = visible_keys(&data, (2, 4), 5, &seen, |k| *k);
        assert_eq!(keys.first(), Some(&(0, false)));
        assert_eq!(keys.last(), Some(&(8, false)));
    }

    #[test]
    fn visible_keys_excludes_already_seen() {
        let data: Vec<(i32, bool)> = (0..10).map(|i| (i, false)).collect();
        let mut seen = std::collections::HashSet::new();
        seen.insert((3, false));
        seen.insert((5, false));
        let keys = visible_keys(&data, (0, 10), 0, &seen, |k| *k);
        assert_eq!(keys.len(), 8);
        assert!(!keys.contains(&(3, false)));
        assert!(!keys.contains(&(5, false)));
    }

    #[test]
    fn visible_keys_empty_data_yields_empty() {
        let data: Vec<(i32, bool)> = Vec::new();
        let seen = std::collections::HashSet::new();
        let keys = visible_keys(&data, (0, 0), 30, &seen, |k| *k);
        assert!(keys.is_empty());
    }

    #[test]
    fn visible_keys_out_of_range_yields_empty() {
        let data: Vec<(i32, bool)> = (0..5).map(|i| (i, false)).collect();
        let seen = std::collections::HashSet::new();
        // lo = 95, hi = (110 + 5).min(5) = 5 => get(95..5) is an invalid range => &[]
        let keys = visible_keys(&data, (100, 110), 5, &seen, |k| *k);
        assert!(keys.is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test -p ultros-app visible_keys`
Expected: FAIL — `cannot find function visible_keys in this scope` (the helper doesn't exist yet).

- [ ] **Step 3: Implement the helper**

Add this immediately after the `select_enrichment_keys` function (just before `#[component] fn PresetFilterButton`). The `#[allow(dead_code)]` is temporary — Task 3 wires this in and removes it.

```rust
/// Keys in the `[start - margin, end + margin)` slice of `data`, minus `seen`.
/// Generic over the row type + a key extractor so it unit-tests with plain
/// `(i32, bool)` fixtures — no `CalculatedProfitData` / DOM needed. Wired into
/// the lazy-enrichment effect in `AnalyzerTable`.
#[allow(dead_code)] // wired into the fetch effect in the same change set
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

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ultros-app visible_keys`
Expected: PASS — 5 tests pass (`visible_keys_includes_window_and_margin`, `visible_keys_clamps_at_start_and_end`, `visible_keys_excludes_already_seen`, `visible_keys_empty_data_yields_empty`, `visible_keys_out_of_range_yields_empty`).

- [ ] **Step 5: Run the format + lint gate**

Run: `./check_ci.sh`
Expected: passes. (`visible_keys` would warn as dead code, but `#[allow(dead_code)]` suppresses it for this task only.)

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "test(analyzer): add visible_keys window/margin/dedupe helper

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Move enrichment into `AnalyzerTable` (lazy visible-window fetch)

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs`

This task swaps the data-fetch model. It is committed as one unit (the parent removal and child addition must land together to compile). Intermediate steps may not compile on their own; the compile gate is Step 11.

- [ ] **Step 1: Add the `settled` set to `EnrichmentMaps` + an accessor**

Replace the `EnrichmentMaps` struct (currently 3 fields → 2 fields; it has `quality` + `sparkline`) and its impl. Find:

```rust
#[derive(Clone, Debug, Default)]
struct EnrichmentMaps {
    quality: HashMap<(i32, bool), ResaleQualityRow>,
    sparkline: HashMap<(i32, bool), Vec<u32>>,
}

impl EnrichmentMaps {
    fn quality_for(&self, key: &(i32, bool)) -> Option<&ResaleQualityRow> {
        self.quality.get(key)
    }
    fn sparkline_for(&self, key: &(i32, bool)) -> Option<&Vec<u32>> {
        self.sparkline.get(key)
    }
}
```

Replace with:

```rust
#[derive(Clone, Debug, Default)]
struct EnrichmentMaps {
    quality: HashMap<(i32, bool), ResaleQualityRow>,
    sparkline: HashMap<(i32, bool), Vec<u32>>,
    /// Keys whose fetch has completed (with OR without data). Lets cells tell
    /// "still loading" (absent) from "fetched, no CH data" (present, but no
    /// entry in `quality` / `sparkline`).
    settled: std::collections::HashSet<(i32, bool)>,
}

impl EnrichmentMaps {
    fn quality_for(&self, key: &(i32, bool)) -> Option<&ResaleQualityRow> {
        self.quality.get(key)
    }
    fn sparkline_for(&self, key: &(i32, bool)) -> Option<&Vec<u32>> {
        self.sparkline.get(key)
    }
    fn is_settled(&self, key: &(i32, bool)) -> bool {
        self.settled.contains(key)
    }
}
```

- [ ] **Step 2: Add imports + constants; drop the temporary `allow`**

(a) Change the skeleton import (currently `skeleton::BoxSkeleton,`) to:

```rust
        skeleton::{BoxSkeleton, SingleLineSkeleton},
```

(b) Add the `TimeoutFuture` import near the top-of-file `use` statements (e.g. just after the `use chrono::...;` line):

```rust
use gloo_timers::future::TimeoutFuture;
```

(c) Remove the `#[allow(dead_code)] // wired into the fetch effect in the same change set` line above `fn visible_keys` (it is used for real now).

(d) Add the tuning constants next to `visible_keys` (just above it):

```rust
/// Rows fetched above & below the rendered window, so enrichment lands just
/// before a row scrolls into view. Keep small enough that
/// `rendered (~26) + 2 * PREFETCH_MARGIN` stays well under the 200-item
/// sparklines cap (no chunking needed).
const PREFETCH_MARGIN: usize = 30;
/// Debounce window for scroll-driven fetches (ms). Mirrors search_box.rs.
const DEBOUNCE_MS: u32 = 150;
```

- [ ] **Step 3: Remove the `enrichment` prop from `AnalyzerTable`**

In the `#[component] fn AnalyzerTable(...)` signature, delete this prop (the doc comment + the field):

```rust
    /// CH-backed per-row enrichment (quality band + sparkline). Empty
    /// when the enrichment fetch is in flight or failed — the table
    /// degrades gracefully to Pass-1 rendering.
    enrichment: Signal<EnrichmentMaps>,
```

- [ ] **Step 4: Declare the accumulating `enrichment` signal before `sorted_data`**

`sorted_data`'s suspicious-filter closure reads `enrichment`, so the signal must exist before it. Immediately before the `let sorted_data = Memo::new(move |_| {` line, add:

```rust
    // Accumulating CH enrichment (quality + sparkline + settled), grown by the
    // visible-window fetch effect below; never wholesale-replaced (except on a
    // world change). Cells + the suspicious filter read it reactively.
    let enrichment = RwSignal::new(EnrichmentMaps::default());
```

- [ ] **Step 5: Add the fetch state + effects after `sorted_data`**

`sorted_data` ends with `.collect::<Vec<(usize, CalculatedProfitData)>>()` followed by `});`. Immediately after that closing `});` (and before `view! {`), add:

```rust
    // --- Visible-window lazy enrichment -------------------------------------
    // Dedupe / loop-breaker: keys we've already scheduled a fetch for. Non-
    // reactive (StoredValue) on purpose — claiming a key must not retrigger the
    // fetch effect.
    let requested = StoredValue::new(std::collections::HashSet::<(i32, bool)>::new());
    // Rendered row range published by the VirtualScroller (see view! below).
    let visible_range = RwSignal::new((0usize, 0usize));
    // Generation counter for debounce-with-cancellation (RwSignal, mirroring
    // components/search_box.rs). `gen` is a reserved keyword in edition 2024.
    let fetch_id = RwSignal::new(0u64);

    // Reset accumulated enrichment when the world changes. Defense-in-depth: if
    // the component is updated in place rather than remounted, another world's
    // data must not leak.
    Effect::new(move |_| {
        let _ = world.get(); // subscribe: re-run on world change
        enrichment.set(EnrichmentMaps::default());
        requested.update_value(|s| s.clear());
    });

    // Select the visible-window keys (honoring the active sort/filter via
    // sorted_data), debounce, fetch both batches, and merge — accumulating.
    Effect::new(move |_| {
        let range = visible_range.get(); // reactive: scroll
        let keys = sorted_data.with(|data| {
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
            TimeoutFuture::new(DEBOUNCE_MS).await; // debounce
            if fetch_id.get_untracked() != current_id {
                return; // superseded by a newer range
            }
            // Claim post-debounce so superseded generations never claim.
            requested.update_value(|s| s.extend(keys.iter().copied()));
            // window <= ~86 keys << 200 cap -> single batch, no chunking.
            let (quality, sparklines) = futures::join!(
                get_resale_quality(&world_name, keys.clone(), 30),
                post_sparklines(
                    &world_name,
                    SparklinesRequest {
                        items: keys.clone(),
                        // 7 days; server caps at 168h. Matches prior behavior.
                        hours: Some(168),
                    },
                ),
            );
            // Merge whatever succeeded and mark every fetched key settled
            // (success OR error) so cells switch loading -> value / "—". On a CH
            // blip the rows degrade to "—" (same as today) — no retry loop; a
            // world change resets everything.
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
        });
    });
```

- [ ] **Step 6: Wire `visible_range` into the `<VirtualScroller>`**

In the `view!`, find the `<VirtualScroller` opening tag and add the prop right after `variable_height=false`:

```rust
                <VirtualScroller
                        viewport_height=720.0
                        row_height=40.0
                        overscan=8
                        header_height=56.0
                        variable_height=false
                        visible_range=visible_range
```

- [ ] **Step 7: Update the Trend cell for loading / no-data / value**

Replace the `COL_TREND` cell block. Find:

```rust
                                            {move || visible_cols().contains(COL_TREND).then(|| {
                                                let maps = enrichment.get();
                                                let pts = maps.sparkline_for(&row_key).cloned().unwrap_or_default();
                                                let pct = maps.quality_for(&row_key)
                                                    .map(|q| {
                                                        let vwap = q.vwap as f32;
                                                        if vwap <= 0.0 {
                                                            0.0
                                                        } else {
                                                            (row_cheapest_price as f32 - vwap) / vwap * 100.0
                                                        }
                                                    })
                                                    .unwrap_or(0.0);
                                                view! {
                                                    <div role="cell" class="px-3 py-2 w-[100px] hidden md:flex items-center justify-center">
                                                        <Sparkline points=pts pct_change=pct />
                                                    </div>
                                                }
                                            })}
```

Replace with:

```rust
                                            {move || visible_cols().contains(COL_TREND).then(|| {
                                                let maps = enrichment.get();
                                                let inner = if let Some(pts) = maps.sparkline_for(&row_key) {
                                                    let pct = maps.quality_for(&row_key)
                                                        .map(|q| {
                                                            let vwap = q.vwap as f32;
                                                            if vwap <= 0.0 {
                                                                0.0
                                                            } else {
                                                                (row_cheapest_price as f32 - vwap) / vwap * 100.0
                                                            }
                                                        })
                                                        .unwrap_or(0.0);
                                                    view! { <Sparkline points=pts.clone() pct_change=pct /> }.into_any()
                                                } else if maps.is_settled(&row_key) {
                                                    // fetched, no series -> empty sparkline (prior behavior)
                                                    view! { <Sparkline points=Vec::new() pct_change=0.0 /> }.into_any()
                                                } else {
                                                    view! { <SingleLineSkeleton /> }.into_any()
                                                };
                                                view! {
                                                    <div role="cell" class="px-3 py-2 w-[100px] hidden md:flex items-center justify-center">
                                                        {inner}
                                                    </div>
                                                }
                                            })}
```

- [ ] **Step 8: Update the Sales/day and 30d-Volume cells**

Replace the `COL_SALES_PER_DAY` block. Find:

```rust
                                            {move || visible_cols().contains(COL_SALES_PER_DAY).then(|| {
                                                let text = enrichment.get()
                                                    .quality_for(&row_key)
                                                    .map(|q| format!("{:.1}", q.sales_per_day))
                                                    .unwrap_or_else(|| "—".to_string());
                                                view! {
                                                    <div role="cell" class="px-3 py-2 w-[88px] hidden md:flex items-center justify-end font-mono tabular-nums">
                                                        {text}
                                                    </div>
                                                }
                                            })}
```

Replace with:

```rust
                                            {move || visible_cols().contains(COL_SALES_PER_DAY).then(|| {
                                                let maps = enrichment.get();
                                                let inner = match (maps.quality_for(&row_key), maps.is_settled(&row_key)) {
                                                    (Some(q), _) => view! { {format!("{:.1}", q.sales_per_day)} }.into_any(),
                                                    (None, true) => view! { "—" }.into_any(),
                                                    (None, false) => view! { <SingleLineSkeleton /> }.into_any(),
                                                };
                                                view! {
                                                    <div role="cell" class="px-3 py-2 w-[88px] hidden md:flex items-center justify-end font-mono tabular-nums">
                                                        {inner}
                                                    </div>
                                                }
                                            })}
```

Replace the `COL_VOLUME_30D` block. Find:

```rust
                                            {move || visible_cols().contains(COL_VOLUME_30D).then(|| {
                                                let text = enrichment.get()
                                                    .quality_for(&row_key)
                                                    .map(|q| q.sample_size.to_string())
                                                    .unwrap_or_else(|| "—".to_string());
                                                view! {
                                                    <div role="cell" class="px-3 py-2 w-[88px] hidden md:flex items-center justify-end font-mono tabular-nums">
                                                        {text}
                                                    </div>
                                                }
                                            })}
```

Replace with:

```rust
                                            {move || visible_cols().contains(COL_VOLUME_30D).then(|| {
                                                let maps = enrichment.get();
                                                let inner = match (maps.quality_for(&row_key), maps.is_settled(&row_key)) {
                                                    (Some(q), _) => view! { {q.sample_size.to_string()} }.into_any(),
                                                    (None, true) => view! { "—" }.into_any(),
                                                    (None, false) => view! { <SingleLineSkeleton /> }.into_any(),
                                                };
                                                view! {
                                                    <div role="cell" class="px-3 py-2 w-[88px] hidden md:flex items-center justify-end font-mono tabular-nums">
                                                        {inner}
                                                    </div>
                                                }
                                            })}
```

- [ ] **Step 9: Delete the parent's eager fetch + the `enrichment` prop pass**

(a) In `AnalyzerWorldView`, delete the entire eager-enrichment block — the three `*_for_enrichment` clones, the `LocalResource`, and the derived `enrichment_signal`. Find and remove:

```rust
    let sales_for_enrichment = sales.clone();
    let world_cheapest_for_enrichment = world_cheapest_listings.clone();
    let global_cheapest_for_enrichment = global_cheapest_listings.clone();
    let enrichment = LocalResource::new(move || {
        let world_name = world();
        let sales_res = sales_for_enrichment.get();
        let world_cheapest_res = world_cheapest_for_enrichment.get();
        let global_cheapest_res = global_cheapest_for_enrichment.get();
        let filter_outliers = filter_outliers().unwrap_or(false);
        async move {
            // ... entire async body ...
        }
    });
    let enrichment_signal: Signal<EnrichmentMaps> =
        Signal::derive(move || enrichment.get().unwrap_or_default());
```

(Delete from `let sales_for_enrichment = sales.clone();` through the end of the `enrichment_signal` `Signal::derive(...)` statement — i.e. the whole comment block at `// CH enrichment:` plus those bindings.)

(b) In the `<AnalyzerTable .../>` usage, delete the prop line:

```rust
                                                    enrichment=enrichment_signal
```

- [ ] **Step 10: Delete the now-dead ranking helpers + caps**

These were only used by the deleted `LocalResource`; clippy `-D warnings` will flag them as dead code. Remove:
- the `ENRICHMENT_BATCH_CAP` const + its doc comment,
- the `SPARKLINE_BATCH_CAP` const + its doc comment,
- `fn roi_for_rank(...)` + its doc comment,
- `fn select_enrichment_keys(...)` + its doc comment.

(Keep `EnrichmentMaps`, `ProfitTable`, `get_resale_quality`, `post_sparklines`, `SparklinesRequest`, `ResaleQualityRow`, `ConfidenceBand` — all still used by the new `AnalyzerTable` code.)

- [ ] **Step 11: Compile, lint, and run unit tests**

Run: `cargo test -p ultros-app visible_keys`
Expected: PASS — the 5 `visible_keys` tests still pass (helper unchanged).

Run: `./check_ci.sh`
Expected: passes — fmt clean; clippy clean (no dead-code warnings; the `#[allow(dead_code)]` from Task 2 is gone and `visible_keys` is now used). If clippy flags a leftover unused import or binding, remove it.

- [ ] **Step 12: Manual browser verification**

Run the SSR app locally (see project notes / `reference_ultros_local_browser_test`: jemalloc/MSVC build wall + `bin-features=[]` workaround, ClickHouse creds) and open the analyzer on a busy world (e.g. a high-traffic NA/EU world). Confirm:
1. Top rows populate Trend / Sales-day / 30d-Volume on load (no separate eager pass needed).
2. Scrolling down fills previously-blank rows; in-flight rows briefly show the thin `SingleLineSkeleton` pulse, then a value or `—`.
3. The Network panel shows **small batched** `POST /api/v1/resale_quality/...` + `POST /api/v1/sparklines/...` per scroll-stop (~one pair), not one request per row.
4. Scrolling back up does **not** re-issue requests for already-seen rows.
5. Switching sort (ROI ↔ Profit ↔ Profit-per-day) keeps already-fetched values (no refetch storm), and newly-surfaced rows fetch.
6. Switching world clears and refetches for the new world.

(Rows with no ClickHouse rollup still show `—` for Sales-day / 30d-Volume — that's the separate, out-of-scope `item_stats_window` backfill coverage issue; Trend from `sales_hourly` should still populate.)

- [ ] **Step 13: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "feat(analyzer): visible-window lazy enrichment

Fetch Trend / Sales-day / 30d-Volume for rows in (and near) the viewport,
accumulating as the user scrolls, instead of capping at the top 240/200 ROI
rows. Reuses the existing bulk resale_quality + sparklines endpoints; debounced
generation-guard fetch mirrors search_box.rs. Cells show a skeleton while in
flight. Removes the parent eager-fetch path and the ROI ranking helpers.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage** (each spec section → task):
- §1 VirtualScroller `visible_range` prop → Task 1. ✓
- §2 `AnalyzerTable` ownership: `EnrichmentMaps.settled` → T3 S1; `visible_keys` helper → T2; state signals + reset effect + debounced fetch effect → T3 S4-S5; `visible_range` wiring → T3 S6; constants/imports → T3 S2. ✓
- §3 Cell loading/no-data/value with `SingleLineSkeleton` → T3 S7-S8. ✓
- §4 Caching/invalidation (world reset; sort/filter reuse) → T3 S5 (reset effect; reuse is inherent in `requested` + accumulating map). ✓
- Removal of eager path (`LocalResource`, `enrichment_signal`, prop, `roi_for_rank`, `select_enrichment_keys`, two caps) → T3 S3, S9, S10. ✓
- Testing (unit test for `visible_keys`; `./check_ci.sh`; manual scroll check) → T2 S1-S4, T3 S11-S12. ✓
- Non-goals respected: no backend/API-type/locale changes in any task. ✓

**2. Placeholder scan:** No "TBD"/"TODO"/"handle edge cases"/"similar to" — every code step shows full code; every test step shows assertions; every run step shows the command + expected result. ✓

**3. Type consistency:**
- `EnrichmentMaps` field `settled: std::collections::HashSet<(i32, bool)>` (T3 S1) is read via `is_settled` in cells (T3 S7-S8) and written via `m.settled.extend(...)` in the effect (T3 S5). ✓
- `visible_keys<T>(data, range, margin, seen, key_of)` signature (T2 S3) matches the call in the effect (T3 S5), passing `|(_, d)| (d.inner.sale_summary.item_id, d.inner.sale_summary.hq)` against `sorted_data`'s `Vec<(usize, CalculatedProfitData)>`. ✓
- `visible_range: RwSignal<(usize, usize)>` declared in `AnalyzerTable` (T3 S5), passed to the `visible_range` prop on `VirtualScroller` (T1 S1 prop type `Option<RwSignal<(usize, usize)>>`, `into`). ✓
- `fetch_id: RwSignal<u64>` uses `.update(|n| *n += 1)` / `.get_untracked()` — matches `search_box.rs` precedent. ✓
- Response field access matches api-types: `ResaleQualityResponse.rows` → `ResaleQualityRow { item_id, hq, sales_per_day, sample_size, vwap }`; `SparklinesResponse.series` → `SparklineSeries { item_id, hq, points }`. ✓
- `get_resale_quality(&str, Vec<(i32,bool)>, u16)` and `post_sparklines(&str, SparklinesRequest)` signatures match `api.rs`. ✓

**4. Edition-2024 / build gotchas covered:** `gen` keyword avoided (`fetch_id`); `gloo-timers` is a plain dep that compiles under `ssr`; `default = ["ssr"]` so `cargo test -p ultros-app` runs natively; submodule + Perl + `CARGO_TARGET_DIR` called out in Prerequisites. ✓

No gaps found.
