# Analyzer Kit Phase E2: The Flip Finder's Column Family on the Recipe Analyzer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The recipe analyzer's Labs experiment becomes **one** toggle — `analyzer-recipe`, replacing `analyzer-ledger` (Phase C) and `analyzer-signal-columns` (Phase D) — and under that one toggle the tool gains Profit/day, a lazy 7-day Trend sparkline with its Drift, Volume (30d) and VWAP (30d) from a client-only 30-day body, "7d · ‹sell world›" sub-labels and tooltips on Daily sales and Confidence, a signed "vs median" tell under Price, and Market / Location groups in the Columns picker. With the toggle off, every URL renders, fetches and computes exactly as it did before Phase C.

**Architecture:** The kit grows its lazy layer: `Layer::Lazy(LazyFeed::Sparklines { hours })`, `Sortability::LazyNever`, `Enrich<V> { Loading, Missing, Ready(V) }`, four lazy/late `CellValue`s rendered in one shape each, a `SparkStore` (the E1 `Enrichment` over `(item, stat hq)` → `SparkValue { points, delta_pct }`), `Enrichment::state`, and an `AnalyzerGrid` `visible_range` prop. Lazy and late cells reach their page-level data through two `Copy` signal handles on `CellCtx` (`sparklines`, `stats_30`) so cell extractors stay `fn` pointers and `render_cell` stays the only place per-variant markup lives. The page owns a `MarketHandles` bundle (sparkline store, 30-day body, `visible_range`, a rows mirror); the E1 hook runs at page level over the mirror, so a table remount keeps every settled key and only a sell-world change resets. The 30-day body is one `Effect` + `spawn_local` per sell world into `RwSignal<Option<Arc<StatsIndex>>>`, never under Suspense; sorting on a 30d column before it lands falls back to Profit. Lab gating does not change shape: one token gates all fifteen optional columns, so the grid's `lab_columns: bool`, the two `?cols=` contracts and `SortMode::lab_only` stay exactly as Phase D built them.

**Tech Stack:** Rust 2024, Leptos 0.8.20 / reactive_graph 0.2.14 / tachys 0.2.18 (SSR + hydrate), leptos_i18n 0.6 (seven locales), the analyzer kit (`ultros-frontend/ultros-app/src/analyzer_kit/`), `ultros-api-types`.

**Specs:** `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md` — §3 module table and core types (L107–224), §5 catalog rows for ProfitPerDay / RevenueSlot / VolumeUnits 30d / Vwap 30d / DriftBuffer / Trend and the picker groups (L270–303), §6 Bulk / Lazy / capacity (L305–355), §8 Phase E2 (L417–423) and the variant ledger (L373–379), §9 URL and i18n (L458–469), §10 decision points 3/4/5/7 (L478–487), §11 Labs (L499–521). Reader reports (scratchpad `phase-e2/`): `kit-spec-e2.md`, `flip-finder-column-family.md`, `recipe-analyzer-after-d.md`, `kit-lazy-layer-needs.md`, `phase-0-measurements.md`, `e1-review-advice-for-e2.md`. Line numbers below are against HEAD `a038fed0` (`main` with Phase E1 merged as #1260). **The branch base is `a038fed0` plus the container-mode row-clip fix** (Global Constraints, below), which shifts `recipe_analyzer.rs` by about +18 lines after `:900` and `grid.rs` after `:470`, and they shift again as tasks land — search for the quoted code, never trust an offset.

## Global Constraints

- Every user-facing string goes through `leptos-i18n`; every new key exists in **all seven** locale files (`en, fr, de, ja, cn, ko, tc`) with a real translation (CLAUDE.md). A key missing from a non-default locale only *warns* and falls back to en, so the seven-locale check is Task 1's `grep -c` / key-count step, not a green build.
- `./check_ci.sh` (fmt-check + `cargo clippy --all-targets -- -D warnings`) must exit 0 before the PR; **no `#[allow(dead_code)]`**. Read its exit code from a file, never through a pipe: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"`. On Windows, Strawberry Perl must lead `PATH` (`export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"`).
- Under `pub(crate)` modules and `-D warnings`, any field, fn, variant or `pub use` whose only readers are tests fails CI. Kit items are dead **between** tasks by design (Task 3's `Enrich`, `SparkStore`, `LazyFeed` have no production reader until Task 7); the branch-level gate is `check_ci.sh` in Task 10. Each task's own gate is `cargo test -p ultros-app --lib -- <filter>`, which tolerates dead-code warnings.
- **Toggle off = byte-identical.** One flag now gates the whole tool, so there is exactly one flag-off state to pin: with `analyzer-recipe` off, every URL renders the same DOM, issues the same requests and computes the same numbers as it did *before Phase C* — with one carve-out: the row-clip fix this branch stacks on adds `min-w-max` to the grid header class and `min-width: max-content` to the row spacer on that page, for every user, in its own PR. The mechanisms, each pinned by a test: `BASE_COLUMN_ORDER` hands `parse_visible_cols` a contract with no lab token; `SortMode::lab_only` drops all thirteen lab sorts; `AnalyzerGrid`'s `lab_columns=false` drops lab columns from the header at build time (a hidden optional column still writes a `<!>` marker, so `?cols=` filtering alone is not enough; the page remounts the table on a flip because the Suspense join reads the lab); the flat picker excludes every `lab.is_some()` column; `HeaderExtras` is an empty map; `CellCtx.preview = false` keeps `cell_price` on `CellValue::Gil`; and both E2 fetches (the sparklines POST, the 30d body) are gated on tokens that cannot be visible or sorted with the toggle off.
- **Numbers: none on existing columns.** Every existing cell, sort and fetch is unchanged; the recorded oracle (`price_rows_matches_recorded_oracle_on_fixture`) must not move. The two refactors that touch existing numbers are pure: `profit_per_day` delegates to `profit_per_day_from_rate` (same arithmetic, pinned by its three tests) and `price_rows`' sell-stat lookup becomes `stat_row_either` (same two-step rule).
- The flip finder changes in exactly one way — its Drift cell's colour class comes from `signed_delta_class` — and that change is byte-identical (Task 2 shows the format equivalence). Nothing else on `/flip-finder` moves.
- **No HashMap iteration order may reach the DOM** (hydration): the sparkline store, the 30d index and `HeaderExtras.by_kind` are looked up by key only; the sparkline fetch collects a `Vec` that feeds a map, never a view.
- **One shape per resource-backed cell.** Each new `CellValue` variant renders the same element list across `Loading` / `Missing` / `Ready`: a skeleton bar that shows or hides by class, and a value slot that mutes by class. The one honest exception, stated in the shape test: the Trend cell's `Ready` state adds SVG children inside its fixed span (the `Sparkline` component draws an `<svg>` only when it has points); `Loading` and `Missing` are shaped alike, and `Loading` is what both the server and the first client paint render (the stores are empty on both sides), so hydration never sees `Ready`.
- **Client-only bodies stay in `Effect` bodies.** `post_api`'s SSR arm is `unreachable!` (`api.rs:1196-1202`) and the 30d `fetch_api` must not join the Suspense gate. Effect bodies never run under `leptos/ssr` (`Effect::new` only spawns when `reactive_graph/effects` is on — a runtime `cfg!`, so the bodies still compile on the server). The sparklines fetch goes through the E1 hook only; the 30d fetch is one `Effect::new` + `leptos::task::spawn_local` at page level. No `Resource`, `LocalResource`, memo or `Suspense` may call either. Both stores are `None` / empty on the server and on the first client paint.
- **`try_*` on every signal touched after an await** (`sell_world_name`, `stats_30`, the in-flight flag): a plain read of a disposed signal panics and takes the wasm bundle down. The guard is the kit's `verdict(sig.try_get_untracked(), &captured)`.
- **The E1 hook is not changed**: `use_visible_enrichment` keeps its seven parameters (`too_many_arguments` fires above seven) and its mount-time reset; the page-level requirement is met by calling it at page level (Decisions).
- **No new URL selection key.** `?cols=` gains exactly `profit-per-day, trend, drift, volume-30d, vwap-30d`, appended after the seventeen existing tokens; `?sort=` gains `profit-per-day, volume-30d, vwap-30d` (no sort token for `trend` or `drift`). `SortMode` gains three variants for **24** distinct modes; the table has **22** optional ids. `DEFAULT_COLS` stays `["confidence"]`; `migrate_legacy_params` is untouched. The `?labs=` values `analyzer-ledger` and `analyzer-signal-columns` stop being known tokens (Task 1).
- New columns keep the page's `hidden md:` convention (kit decision 7). A two-line header uses `hidden md:flex`, never `hidden md:block` (a later `md:block` overrides the appended `flex flex-col` at md+).
- Plain-key `t_string!(i18n, key)` is `&'static str`; only an interpolated key returns a builder that needs `.to_string()`. Never `&t_string!(..)` in a `&str` position.
- Run `cargo` in the **foreground** inside subagents. No bare `git stash`. Do **not** post anything to Kosyne on #1233; Aaron validates on the PR.
- Branch `claude/issue-1233-phase-e2-market-columns`, cut from `main` at `a038fed0`. The PR targets `main`; rebase onto `origin/main` before opening it so CI runs (a PR whose base is not `main` gets none).
- **E2 stacks on the container-mode row-clip fix and must not redo it.** A separate PR (branch `claude/issue-1233-grid-row-clip`) fixes a shipped bug this plan would otherwise make worse: in `VirtualScroller`'s Container mode the rows are clipped at the viewport width while the sticky header, a sibling outside both clips, paints to the full grid width — so a table wider than the viewport shows headers over blank rows, and any viewport narrower than the table loses every cell you scroll to — captured on prod at 820x900 (scroller and row wrapper both `scrollWidth` 1968, `clientWidth` 756), where the cells past the port are not merely unpainted but unhittable (`elementFromPoint` returns the scroller, so a row's buttons are unclickable). That fix (a) makes the row-area class mode-dependent so Container mode carries no `overflow` pair, (b) adds a `row_min_width` pass-through on `AnalyzerGrid` forwarded to `VirtualScroller`, and (c) has the recipe analyzer pass `row_min_width="max-content"` with `min-w-max` on its `header_class`. Consequences for this plan: cut or rebase this branch onto that fix (or onto `main` once it merges) before Task 1; `AnalyzerGrid` already has a `row_min_width` prop, so Task 5 **adds** `visible_range` beside it rather than introducing the first new prop; the `<AnalyzerGrid>` call in `recipe_analyzer.rs` already carries `row_min_width` and a `min-w-max` header class, so quote it as it then stands; and the five columns this plan adds (`w-32` × 2 + `w-28` × 3 = 592 px) need no further width work because `max-content` follows them automatically. Do not re-apply any part of that fix here.

## Decisions taken in this plan

| Question | Decision |
|---|---|
| How many Labs flags does the recipe analyzer end with? | **One.** Aaron, mid-plan: "multiple permutations of the feature is a little much. Can we prune it back to just a single toggle for the recipe analyzer?" So `LAB_ANALYZER_RECIPE = "analyzer-recipe"` replaces both `analyzer-ledger` (C) and `analyzer-signal-columns` (D), E2 ships under it, and no third token is created (the spec's `analyzer-market-columns`, §11 L512, never exists). One `LABS` entry, one Settings title/description pair, one `use_lab` call threaded as one signal. `ToolColumnMeta.lab` is `Some(LAB_ANALYZER_RECIPE)` on all fifteen optional lab columns, so the grid's `lab_columns: bool` stays a plain bool and Phase D's `lab_columns_are_absent_from_the_header_unless_enabled` keeps its meaning. **No token aliases:** a bookmarked `?labs=analyzer-ledger` or `?labs=analyzer-signal-columns`, and any stored `LABS` cookie holding them, stop working — `Labs::from_str` drops unknown tokens — and the only affected users (Aaron and testers) re-toggle once in Settings. |
| Flag cap after E2 | The recipe analyzer has exactly **one** flag, so `LABS.len() == 1` and `the_experiment_list_stays_short` (≤ 3) has room again: **Phase F's sell scope ships under this same toggle and retires nothing.** The spec's per-phase token list (§11 L511–513) is corrected in Task 10. |
| Drift: recentSales body (spec §8, decision point 3) or the lazy sparkline delta? | **The downgrade.** Measured 2026-09-03 on prod: `GET /api/v1/recentSales/Gilgamesh` is 9,035,358 B raw / 1,170,030 B on the wire / 1.6 s — 9 MB in the client for one optional column is not acceptable. Drift is `first_to_last_pct(first_price, last_price)` from the same `SparklineSeries` the Trend feed already fetches: lazy, unsortable, with the flip finder's ±1% dead band and its `text-emerald-300` / `text-red-300` classes. `ColumnKind::DriftSpark` names that definition (the spec's `DriftBuffer` stays for a future body-backed variant). No `raw_sales_key` change, no `drift_needed`, no `(item, hq)` buffer index, no `?sort=drift`. |
| Profit/day (decision point 4) | **Opt-in**, `Layer::Computed`, `profit_per_day_from_rate(profit, daily_sales)` computed in the extractor and the comparator (no row field), sortable Desc, i32 truncation. |
| 30d columns (decision point 5) | **Ship default-off and watch the cache metric.** Measured: `GET /api/v1/sale_stats/Gilgamesh?window=30` is 3,250,000 B raw / 437,759 B wire / 0.7 s (the spec's estimate was 2.3 MB / 300 KB). One client-only body per sell world in `RwSignal<Option<Arc<StatsIndex>>>` (`signals::LateStats`), fetched by a page `Effect` iff `needed_bodies` contains `SellWorldStats(30)` (a 30d column visible or the sort target), kept across column toggles, dropped on a sell-world change, `None` on the server and first paint. A failed fetch stores an **empty index** so cells settle to "—" instead of shimmering (no retry until the world changes; declared). Sorting on `volume-30d` / `vwap-30d` **falls back to Profit** (`effective_sort_mode`) until the body lands **with rows** — an empty index counts as not-landed for sorting, because otherwise every key compares Equal and the key-id tiebreak leaves the table in recipe-id order, exactly what the fallback exists to prevent — and the table re-sorts the moment real rows arrive. |
| Phones (decision point 7) | **Keep `hidden md:` through F.** All five new columns are `hidden md:block` / `hidden md:flex`, and every two-line header is `md:flex` (never `md:block`, which would override the appended `flex flex-col` at md+). The hscroll port is Phase G. |
| Where the 30d values are read | **At cell and sort time, not on the row.** tachys' keyed `<For>` (`keyed.rs` `apply_diff`) moves a same-key item and never rebuilds it, so a re-priced row with the same `(index, key_id)` would keep its skeleton forever; the flip finder's cells react because they read the store *inside* the row closure. So `cell_volume_30` / `cell_vwap_30` read `CellCtx.stats_30` (a `Copy` signal handle) inside the row closure, and `filter_and_sort` / `compare_recipes` take `stats_30: Option<&StatsIndex>`. The row only carries `stat_hq`, the key both lookups use. |
| How lazy cells reach the sparkline store | **A typed handle on `CellCtx`**: `sparklines: Option<RwSignal<SparkStore>>` (and `stats_30: Option<LateStats>`), `None` on every other page and in tests. `RwSignal` is `Copy + Eq + Hash` for any `T`, and `Debug` needs only `S: Debug` (`reactive_graph-0.2.14/src/signal/rw.rs:252-292`; `SyncStorage` derives `Debug`), so `CellCtx`'s derives hold. The table stays declarative (`cell: cell_trend`), extractors stay `fn` pointers, `render_cell` keeps every variant's markup (kit §3 hydration invariant), and `HeaderExtras` / the picker key on `ColumnKind` as before. The cost is a store read inside the row closure, so an enrichment merge re-renders every mounted row (the spec accepts this, L212–214). Phase G's composite `FlipEnrichment` will need its own handle or a generalisation; declared. |
| Hook placement (page vs table) | **The hook runs at page level.** The table writes its sorted rows into a page-owned `rows: RwSignal<Vec<(usize, RecipeRow)>>` from one `Effect` (empty unless Trend or Drift is visible, so the toggle-off page never fetches), the page owns `visible_range` and passes it through the grid's new prop, and the hook's `scope` is `sell_world_name`. A table remount (a cost-basis switch) therefore keeps the hook's `requested` set and the store; a world change resets both. The alternative (hook in the table, first-run no-op reset, `requested` seeded from `settled`) was rejected: during the Suspense gap of a world change the hook is unmounted, so the remounted hook's first run would find the *old* world's series under the same `(item, hq)` keys and, with the seed, never refetch them. |
| Picker groups | `PickerGroup::{Market, Location}` inserted before `Other`; the seven older optional columns move (confidence, last-sold, volume, vwap, tax → Market; listing-world, listing-dc → Location); the five new columns are Market. **`Other` stays**: the always-on columns (Item, Profit, ROI, Cost, Price, Daily sales, Avg price, Actions) need a group, and none of them reaches the picker, so the grouped picker ends with five headings — Revenue, Cost, Travel, Market, Location. No collapse rule and no `PickerContext` change are needed: with one flag the grouped picker only ever renders while the toggle is on. |
| "· 7d" sub-labels without a pill | `HeaderLine2.pill` becomes `Option<HeaderPill>`; `HeaderExtra` gains `header_class: Option<&'static str>` so Daily sales (`HEAD_MD`) and Confidence (`HEAD_28_MD`) can switch to a two-line `md:flex` class only while the extra is in effect (a static class change would alter the toggle-off DOM). The unsortable header arm (`Sortability::No | LazyNever`) learns to render a title and line 2 from `HeaderExtras`; with no entry it is today's markup verbatim. Line 2 reads `"7d · ‹sell world›"` (window and source, kit decision 7) on Daily sales, Confidence, Trend and Drift; the 30d columns carry the window in their label and get a title only. |
| `LazyFeed::Sparklines { hours }` needs a production reader | `impl LazyFeed { pub fn hours(self) -> u16 }`, read by the recipe fetch: `SparklinesRequest { hours: Some(RECIPE_TREND_FEED.hours()) }`, with `const RECIPE_TREND_FEED: LazyFeed = LazyFeed::Sparklines { hours: 168 }` shared by the Trend / Drift rows and the fetch. |
| `Enrich<V>`'s phase (E1 plan L30 vs L1307 disagree) | **E2**, in `cells.rs` (spec §3). Constructed by `Enrichment::state` (kit), the recipe extractors and the tests; read by `render_cell`. Only `map` and `is_loading` are added — nothing without a non-test reader. |
| Sparkline payload | `SparkValue { points: Vec<u32>, delta_pct: Option<f32> }` with `Absorb` = `*self = newer` (E1 plan L32), not the spec's `(Arc<[u32]>, f32)`: `<Sparkline points: Vec<u32>>` takes a `Vec` (the flip finder `to_vec()`s per render anyway) and the colour driver is optional (both ends of the window must have a trade). |
| The lazy skeleton | An inline, class-toggled bar (`skeleton-block skeleton-shimmer w-full h-3 rounded-md`), **not** `SingleLineSkeleton`: one shape needs the element present in every state, and `SingleLineSkeleton`'s `sr-only` "Loading…" would then be announced on settled rows. Same visual as the flip finder's cell skeleton, one fewer `<div>`. |
| `signed_delta_class` fold | The flip finder's Drift cell takes its **class** from `signed_delta_class(row_drift, DELTA_DEAD_BAND_PCT)`; its text stays `{d:+.0}%` / "—" (`format!("+{d:.0}%")` and `format!("{d:.0}%")` are `{d:+.0}%` for the ranges they guarded, so the output is byte-identical). The other five copies (market pulse, recently viewed, movers, trends, related items) use different dead bands or decimals and stay for G/H. `first_to_last_pct` is new in `analysis.rs`. |
| The Price median tell | `CellNote::VsMedian { listing: bool, pct: f32 }` — one note line carrying both tells, `‹listing · ›vs median ±n%`, so the D-era "listing" tell keeps its wording and position. **Orientation: the percent describes the Price, not the median** — `delta_pct(alt, input)` is `(alt - input) / input`, and this sub-line sits under Price, so the call is `delta_pct(Some(market_price), median)` and a price below the median reads negative and red. (`rev_alt_cell` passes the alternative as `alt` because there the alternative is what the cell renders.) Its colour comes from `signed_delta_class(Some(pct), DELTA_DEAD_BAND_PCT)` over a geometry-only `SUB_LINE_GEOM`; in the dead band that composes to exactly today's `SUB_LINE` string, so `CellNote::None` / `ListingFallback` still render byte-for-byte what Phase D shipped (asserted in Task 4). |
| The 30d lookup quality | `stat_row_either(index, item, prefer_hq)` (kit `signals.rs`) is the two-step rule `price_rows` already applies to the 7d body; the row records the resolved `stat_hq`, and the 30d lookups start from it. |
| i18n | **16 new keys, 4 deleted.** New: one labs title/desc pair for the merged toggle, two picker headings, the two 30d labels in the recipe's "(7d)" convention (`Volume (30d)`, not the flip finder's "30d Volume"), **its own Drift label**, seven recipe-specific tooltips (the flip finder's `analyzer_tooltip_*` describe 30-day resale-quality semantics), the "7d" window word and the "vs median {{pct}}" tell. Deleted: `labs_analyzer_ledger_title/_desc` and `labs_analyzer_signal_columns_title/_desc`. Reused verbatim: `analyzer_col_profit_per_day`, `analyzer_col_spark`, `analyzer_drift_unavailable`, `analyzer_price_listing_fallback`. **Not reused: `analyzer_col_drift`** — fr and de translate it and `analyzer_col_spark` to the same word ("Tendance", "Trend"), so reusing it would put two identically-labelled columns side by side and two identical checkboxes in the Market group; `recipe_analyzer_col_drift` carries a distinct value in every locale and the flip finder's own header is left alone. Every locale goes 1778 → 1790 keys. |
| `visible_range` forwarding | The scroller's prop is `#[prop(optional, into)]` on an `Option`, which strips the `Option` (leptos_macro `component.rs:1033`): an `Option` cannot be forwarded. The grid therefore always hands the scroller a range signal — the page's when given, its own otherwise. The scroller's range writer is one client `Effect` that sets a signal: no DOM, so the toggle-off page is unchanged. |
| Spec doc drift (E1 review advice #6) | Kit §6's "20 rows on SSR" for the flip finder becomes "28 (the 20-row fallback plus overscan 8)", and §11's four-token list becomes the single `analyzer-recipe` token, in this PR (Task 10, docs-only). |
| Measurements | The 30d and recentSales bodies are already measured (above). The sparklines POST for a 79-key recipe window (bytes, ms) is measured with `curl` in Task 10 and recorded in the PR body. |

## File map

| File | Responsibility in this phase |
|---|---|
| `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` | 16 new keys, 4 deleted, two fr values normalised (Task 1). |
| `ultros-frontend/ultros-app/src/global_state/labs.rs` | `LAB_ANALYZER_RECIPE` replaces the two tokens; one `LABS` entry (Task 1). |
| `ultros-frontend/ultros-app/src/routes/settings.rs` | One Labs title/desc arm instead of two (Task 1). |
| `ultros-frontend/ultros-app/src/analysis.rs` | `profit_per_day_from_rate`, `DELTA_DEAD_BAND_PCT`, `signed_delta_class`, `first_to_last_pct` (Task 2). |
| `ultros-frontend/ultros-app/src/routes/analyzer.rs` | The Drift cell's class from `signed_delta_class` (Task 2). |
| `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs` | `CellCtx.preview` rename — which also touches every `CellCtx` literal in `cells.rs`, `grid.rs` and the recipe page (Task 1); `ColumnKind` ×5, `PickerGroup::{Market, Location}`, `LazyFeed`, `Layer::Lazy`, `Sortability::LazyNever`, `CellCtx.{sparklines, stats_30}` (Task 3). |
| `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs` | `Enrich<V>` (Task 3); `CellNote::VsMedian`, `CellValue::{Sparkline, LazyPct, LateCount, LateGilWithPct}` + arms + shape tests (Task 4). |
| `ultros-frontend/ultros-app/src/analyzer_kit/enrichment.rs` | `SparkKey`, `SparkValue`, `SparkStore`, `Enrichment::state` (Task 3). |
| `ultros-frontend/ultros-app/src/analyzer_kit/signals.rs` | `LateStats`, `stat_row_either` (Task 3). |
| `ultros-frontend/ultros-app/src/analyzer_kit/grid.rs` | The `LazyNever` arm (Task 3); `visible_range`, `HeaderLine2.pill: Option`, `HeaderExtra.header_class`, the unsortable-with-extras arms (Task 5). |
| `ultros-frontend/ultros-app/src/analyzer_kit/needed.rs` | `STATS_30_WINDOW_DAYS`, `RecipeNeeds.stats_30`, the `SellWorldStats(30)` rule (Task 6). |
| `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` | The single lab threaded through page, table and `MarketMenu` (Task 1); `stat_hq`, extractors, `SortMode` (+3), comparators, `filter_and_sort(stats_30)`, five table rows, the `?cols=` contract, feed / config / grid consts, URL tests (Task 7); `MarketHandles`, the hook at page level, the 30d Effect, the rows mirror, table wiring (Task 8); header extras, `HEAD_*_2`, the Price tell, the picker groups (Task 9). |
| `integration/runner.cjs` | The two `?labs=` routes collapse to one on `analyzer-recipe` (Task 1); that route gains the five new `?cols=` tokens (Task 10). |
| `ultros-frontend/ultros-app/src/routes/changelog.rs`, `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md` | The changelog entry, the §6 and §11 wording fixes (Task 10). |

## Test commands used below

```bash
cargo test -p ultros-app --lib -- <filter>
cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown
```

Both from the worktree root. The default feature is `ssr`, so `cargo test` compiles the server flavour; the wasm check proves the Effects and the `async fn` fetches compile for the client. Run the wasm check with **no `RUSTFLAGS` in the environment** (an env `RUSTFLAGS` replaces `[build] rustflags` and fakes web-sys errors). SSR-render tests (`to_html()`) that touch `<Gil>`, `<Sparkline>` or `t_string!` stand up the executor and an i18n context first, and any test that creates an `RwSignal` runs inside an `Owner`:

```rust
let _ = any_spawner::Executor::init_futures_executor();
let owner = Owner::new();
owner.with(|| {
    provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
    // ... render / signals ...
});
```

Test counts **at the branch base** (`main` + the row-clip fix, HEAD `7baeaa71`), for the "Expected" lines below: `analysis.rs` 42, `routes::analyzer` 69, `routes::recipe_analyzer` 44, `global_state::labs` 3, and 56 across the kit (`cells` 4, `columns` 5, `enrichment` 12, `formula` 10, `grid` 6, `hop` 4, `needed` 9, `signals` 5, `strip` 1). Two of those come from the row-clip fix rather than `a038fed0`: `analyzer_kit::grid::row_min_width_reaches_the_scrollers_spacer` and `routes::recipe_analyzer::the_grid_call_opts_into_a_sized_row_spacer`. Re-count with `grep -c '#\[test\]'` before trusting any "Expected" line if the base has moved again.

---
### Task 1: One Labs toggle for the whole recipe analyzer, and every new i18n key

**Files:**
- Modify: `ultros-frontend/ultros-app/src/global_state/labs.rs:16-41` (two tokens and two `LABS` entries become one) and `:101-131` (its three tests)
- Modify: `ultros-frontend/ultros-app/src/routes/settings.rs:389-411` (`lab_title` / `lab_desc`: two arms each become one)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs:97-107` (`CellCtx.signal_columns` → `preview`, and the doc line at `:101` naming the retired token), `:438` (the test literal) and `:518, :524, :530, :536` (the `PICKER` fixture's `lab: Some("analyzer-signal-columns")` → `Some("analyzer-recipe")`)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs:259, :310` (test literals)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/grid.rs:490, :532, :574, :764, :784, :813` (six test literals — the pair at `:764`/`:784` came with the row-clip fix) and `:617, :655` (the same fixture `lab:` strings)
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` — the import at `:33`, the two comments naming a retired token at `:344` and `:532`, `MarketMenu`'s prop at `:354-361` and its two readers at `:388, :395`, `cell_price` at `:793-806`, the ten `lab: Some(LAB_ANALYZER_SIGNAL_COLUMNS)` values at `:1079-1207`, the table's two props at `:1944-1966` and its readers at `:2103, :2116, :2134, :2406, :2530, :2597, :2768, :2796, :3105`, the page's two `use_lab` calls at `:3147-3148` and its readers at `:3174, :3180, :3193, :3358, :3501, :3532, :3602, :3656, :3661`, and the test at `:4662`
- Modify: `integration/runner.cjs:79-92, :138-140` (two `?labs=` routes become one)
- Modify: `ultros-frontend/ultros-app/locales/en.json`, `fr.json`, `de.json`, `ja.json`, `cn.json`, `ko.json`, `tc.json` (16 keys added, 4 deleted, two fr values normalised, each file 1778 → 1790 keys)

**Interfaces:**
- Consumes: `use_lab` (`global_state/labs.rs:86`), `ToolColumnMeta.lab` (`analyzer_kit/columns.rs:141`), `AnalyzerGrid`'s `lab_columns` prop (`grid.rs:268`) — all unchanged in shape.
- Produces:
  - `pub const LAB_ANALYZER_RECIPE: &str = "analyzer-recipe";` in `global_state/labs.rs`, the only entry in `LABS`.
  - `CellCtx.preview: bool` (was `signal_columns`), read by `cell_price` in Task 9 and by nothing else.
  - `RecipeAnalyzerTable`'s single `preview: bool` prop (replacing `ledger: Signal<bool>` and `signal_cols: bool`) and `MarketMenu`'s single `preview: bool` prop.
  - The 16 keys below, read by Tasks 3, 4, 7 and 9 — including `recipe_analyzer_col_drift`, the recipe analyzer's own Drift label (fr and de render `analyzer_col_drift` and `analyzer_col_spark` with the same word, so the flip finder's key cannot be reused for a column that sits next to Trend).

- [ ] **Step 1: Write the failing test**

In `labs.rs`'s `mod tests`, replace `labs_cookie_round_trips_known_tokens_only` and `every_lab_token_is_listed_once` with these, and add the third:

```rust
    #[test]
    fn labs_cookie_round_trips_known_tokens_only() {
        let labs: Labs = "analyzer-recipe,bogus,,analyzer-recipe".parse().unwrap();
        assert_eq!(labs.enabled.len(), 1);
        assert!(labs.has(LAB_ANALYZER_RECIPE));
        assert_eq!(labs.to_string(), "analyzer-recipe");
        let empty: Labs = "".parse().unwrap();
        assert!(!empty.has(LAB_ANALYZER_RECIPE));
        assert_eq!(empty.to_string(), "");
    }

    /// The two tokens Phases C and D shipped are gone, not aliased: a
    /// stored cookie or a bookmarked `?labs=` holding one of them parses to
    /// the empty set, and the tester re-toggles once in Settings.
    #[test]
    fn the_retired_analyzer_tokens_no_longer_parse() {
        let old: Labs = "analyzer-ledger,analyzer-signal-columns".parse().unwrap();
        assert!(old.enabled.is_empty(), "{old:?}");
        assert_eq!(old.to_string(), "");
    }

    #[test]
    fn every_lab_token_is_listed_once() {
        let mut tokens: Vec<&str> = LABS.iter().map(|l| l.token).collect();
        tokens.sort_unstable();
        tokens.dedup();
        assert_eq!(tokens.len(), LABS.len());
        assert_eq!(tokens, vec![LAB_ANALYZER_RECIPE]);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ultros-app --lib -- global_state::labs`
Expected: compile error, `cannot find value LAB_ANALYZER_RECIPE in this scope`.

- [ ] **Step 3: Collapse the tokens**

Replace `labs.rs:16-41` with:

```rust
/// The recipe analyzer's market model: the profit formula as a control
/// (kit Phase C), a column per price signal with its "use" pill plus Hop
/// gain and Worlds to visit (Phase D), and the market columns — Profit/day,
/// Trend, Drift, Volume (30d), VWAP (30d) (Phase E2). One token for the
/// whole tool: separate flags per phase made "which permutation am I
/// looking at" a question, and the phases only make sense together.
pub const LAB_ANALYZER_RECIPE: &str = "analyzer-recipe";

pub struct LabInfo {
    pub token: &'static str,
}

/// Every live experiment. Adding one here is what makes it appear in
/// Settings; deleting it is part of shipping the feature. Each entry's
/// comment names when it is deleted (a struct field for that would have
/// no non-test reader, which `-D warnings` rejects).
pub const LABS: &[LabInfo] = &[
    // Deleted in the phase after Aaron has validated the market model on
    // prod, which makes it the recipe analyzer's default (kit §11).
    LabInfo {
        token: LAB_ANALYZER_RECIPE,
    },
];
```

- [ ] **Step 4: One Settings arm per function**

In `settings.rs`, `lab_title` and `lab_desc` each keep exactly one non-`_` arm:

```rust
fn lab_title(i18n: I18nContext<Locale, I18nKeys>, token: &str) -> String {
    match token {
        crate::global_state::labs::LAB_ANALYZER_RECIPE => {
            t_string!(i18n, labs_analyzer_recipe_title).to_string()
        }
        _ => token.to_string(),
    }
}

fn lab_desc(i18n: I18nContext<Locale, I18nKeys>, token: &str) -> String {
    match token {
        crate::global_state::labs::LAB_ANALYZER_RECIPE => {
            t_string!(i18n, labs_analyzer_recipe_desc).to_string()
        }
        _ => String::new(),
    }
}
```

- [ ] **Step 5: Rename `CellCtx.signal_columns` to `preview`**

In `analyzer_kit/columns.rs`, the field and its doc:

```rust
/// Per-render context a cell extractor may read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CellCtx {
    pub now_unix: i64,
    /// The page's Labs toggle (`analyzer-recipe` on the recipe analyzer):
    /// the Price slot renders its note sub-line only under it.
    pub preview: bool,
    /// Cost signals the sub-craft cap left unpriced, by
    /// `PriceSignal::index`; their cells render "—" with the cap title.
    pub capped_cost: [bool; 4],
}
```

Then **every** `CellCtx { .. }` literal — ten on this branch, found with `grep -rn 'CellCtx {' ultros-frontend/ultros-app/src/` (which also matches the struct definition at `columns.rs:99`): one in `columns.rs` (`:438`), two in `cells.rs` (`:259`, `:310`), six in `grid.rs` (`:490, 532, 574, 764, 784, 813` — the pair at `:764`/`:784` belongs to `row_min_width_reaches_the_scrollers_spacer`, which the row-clip fix added) and one in `recipe_analyzer.rs` (`:2784`). The kit's nine are mechanical, and the same pass retires the fixture token strings Step 8's gate greps for:

```bash
sed -i 's/signal_columns: false/preview: false/g; s/signal_columns: true/preview: true/g; s/lab: Some("analyzer-signal-columns")/lab: Some("analyzer-recipe")/g' \
  ultros-frontend/ultros-app/src/analyzer_kit/cells.rs \
  ultros-frontend/ultros-app/src/analyzer_kit/columns.rs \
  ultros-frontend/ultros-app/src/analyzer_kit/grid.rs
```

Those six `lab:` values (`columns.rs:518, 524, 530, 536`, `grid.rs:617, 655`) are arbitrary non-`None` markers on synthetic column tables — the kit never imports `global_state::labs` — so the rewrite changes no behaviour; it keeps the fixtures from naming a token that no longer exists. Then fix `columns.rs:101`'s doc line by hand (`The `analyzer-signal-columns` lab:` → `The page's Labs toggle:`, as the field's new doc below already spells it). `recipe_analyzer.rs`'s literal and `cell_price`'s read are covered by Step 6.

- [ ] **Step 6: Thread one signal through the recipe analyzer**

In `routes/recipe_analyzer.rs`, in this order:

0. The two comments naming a retired token, both outside every line range below: `:344` “`/// while the analyzer-ledger lab is on, the stacked formula strip and the`” → “`/// while the analyzer-recipe lab is on, …`”, and `:532` “`// Phase D, behind `analyzer-signal-columns`: appended after the seven`” → “`// Phase D, behind `analyzer-recipe`: appended after the seven`”.
1. Import: `use crate::global_state::labs::{LAB_ANALYZER_RECIPE, use_lab};`
2. `MarketMenu`'s prop and doc (`:354-361`):

```rust
#[component]
fn MarketMenu(
    /// The same ledger chips the inline strip renders, built once on the
    /// page (this component lives inside the table's `ControlBar`).
    terms: Callback<(), Vec<StripTerm>>,
    /// The `analyzer-recipe` Labs toggle. Off = exactly the three selects
    /// below.
    preview: bool,
) -> impl IntoView {
```

   and its two readers become `if preview` (`:388`) and `when=move || preview` (`:395`).

3. `cell_price` (`:794`): `if ctx.signal_columns {` → `if ctx.preview {`.
4. The ten column rows (`:1079-1207`): `lab: Some(LAB_ANALYZER_SIGNAL_COLUMNS)` → `lab: Some(LAB_ANALYZER_RECIPE)`.
5. The table's props: delete `ledger: Signal<bool>` (`:1944-1946`) and replace `signal_cols: bool` (`:1961-1966`) with

```rust
    /// The `analyzer-recipe` Labs toggle: the formula strip and marks, the
    /// clamped ROI, the profit readout, the alternative columns and pills,
    /// the grouped picker, the Price tell, the "n unpriced" note and the
    /// market columns all hang off this one flag. A plain bool: the page
    /// reads the lab inside its Suspense join, so a flip remounts this
    /// table (the grid's header is built once per mount).
    preview: bool,
```

6. The table's readers: `:2103` `if ledger.get() {` → `if preview {`; `:2116` `ledger.get().then(|| {` → `preview.then(|| {`; `:2134` `if !signal_cols {` → `if !preview {`; `:2406` `if signal_cols {` → `if preview {`; `:2530` `ledger.get().then(|| {` → `preview.then(|| {`; `:2597` `if signal_cols && data.unpriced > 0 {` → `if preview && data.unpriced > 0 {`; `:2768` `signal_columns: signal_cols,` → `preview,`; `:2796` `<MarketMenu terms=strip_terms ledger=ledger />` → `<MarketMenu terms=strip_terms preview=preview />`; `:3105` `lab_columns=signal_cols` → `lab_columns=preview`.
7. The page: replace `:3147-3148` with one line,

```rust
    let preview = use_lab(LAB_ANALYZER_RECIPE);
```

   and rewrite its readers: `:3174` `.filter(|m| preview.get() || !m.lab_only())`; `:3180` `if preview.get() {`; `:3193` `if preview.get() {`; `:3358` `preview.get()`; `:3501` `if preview.get() {`; `:3532` `if preview.get() {`; `:3602` `<Show when=move || preview.get()>`; `:3656` delete the `ledger=ledger` prop; `:3661` `signal_cols=signal_cols.get()` → `preview=preview.get()`.
8. The test at `:4662`: `assert_eq!(c.lab, Some(LAB_ANALYZER_RECIPE));`.

Nothing else changes: `SortMode::lab_only`, `BASE_COLUMN_ORDER` / `OPTIONAL_COLUMN_ORDER`, `visible_cols`, `needs_page`, `header_extras`, `column_options` and the grid's `lab_columns` filter all read the one signal where they read one of two before, so the toggle-on page is Phase D's page and the toggle-off page is the pre-Phase-C page.

- [ ] **Step 7: Add the 15 keys and delete the 4 retired ones, in all seven locales**

Write this script to the scratchpad as `phase_e2_keys.py` and run it from the worktree root with `python phase_e2_keys.py`. It inserts each key after the line holding `"signal_short_sale_avg"`, drops the four retired lab keys, keeps each file's indentation and key order, and refuses to run twice.

```python
import io, json, os, re, sys

DELETE = [
    "labs_analyzer_ledger_title",
    "labs_analyzer_ledger_desc",
    "labs_analyzer_signal_columns_title",
    "labs_analyzer_signal_columns_desc",
]

KEYS = {
 "en": {
  "labs_analyzer_recipe_title": "Recipe Analyzer: the market model",
  "labs_analyzer_recipe_desc": "One toggle for the whole tool: the profit formula as a control above the table, a sortable column for every price signal with a “use” pill, Hop gain / unit and Worlds to visit, and the market columns — Profit/day, Trend, Drift, Volume (30d) and VWAP (30d).",
  "analyzer_picker_group_market": "Market",
  "analyzer_picker_group_location": "Location",
  "recipe_analyzer_col_volume_30d": "Volume (30d)",
  "recipe_analyzer_col_vwap_30d": "VWAP (30d)",
  "recipe_analyzer_col_drift": "Drift",
  "recipe_analyzer_window_7d": "7d",
  "analyzer_price_vs_median": "vs median {{pct}}",
  "recipe_analyzer_tooltip_profit_per_day": "Profit per unit times sales per day over the last 7 days on the sell world.",
  "recipe_analyzer_tooltip_trend": "Hourly price on the sell world over the last 7 days. Loaded for the rows in view; the hourly history is not backfilled, so a quiet item can stay blank.",
  "recipe_analyzer_tooltip_drift": "Price change from the start to the end of that 7-day trend. Inside ±1% it reads flat.",
  "recipe_analyzer_tooltip_daily_sales": "Sales per day on the sell world over the last 7 days, from the raw sale count.",
  "recipe_analyzer_tooltip_confidence": "How much 7-day sale history the sell world has for this item: with a small sample, one lucky sale sets the price.",
  "recipe_analyzer_tooltip_volume_30d": "Units sold on the sell world over the last 30 days. Showing it loads a separate 30-day payload, once per world.",
  "recipe_analyzer_tooltip_vwap_30d": "Volume-weighted average price on the sell world over the last 30 days, with its percent against Price.",
 },
 "fr": {
  "labs_analyzer_recipe_title": "Analyseur de recettes : le modèle de marché",
  "labs_analyzer_recipe_desc": "Un seul réglage pour tout l'outil : la formule de profit comme contrôle au-dessus du tableau, une colonne triable par signal de prix avec sa pastille « utiliser », le gain par saut / unité et les mondes à visiter, et les colonnes de marché — bénéfice/jour, tendance, dérive, volume (30 j) et VWAP (30 j).",
  "analyzer_picker_group_market": "Marché",
  "analyzer_picker_group_location": "Emplacement",
  "recipe_analyzer_col_volume_30d": "Volume (30 j)",
  "recipe_analyzer_col_vwap_30d": "VWAP (30 j)",
  "recipe_analyzer_col_drift": "Dérive",
  "recipe_analyzer_window_7d": "7 j",
  "analyzer_price_vs_median": "vs médiane {{pct}}",
  "recipe_analyzer_tooltip_profit_per_day": "Bénéfice par unité multiplié par les ventes par jour des 7 derniers jours sur le monde de vente.",
  "recipe_analyzer_tooltip_trend": "Prix horaire sur le monde de vente au cours des 7 derniers jours. Chargé pour les lignes visibles ; l'historique horaire n'est pas reconstitué, un objet peu échangé peut donc rester vide.",
  "recipe_analyzer_tooltip_drift": "Variation du prix entre le début et la fin de cette tendance sur 7 jours. Dans une marge de ±1 %, elle est affichée comme stable.",
  "recipe_analyzer_tooltip_daily_sales": "Ventes par jour sur le monde de vente au cours des 7 derniers jours, d'après le nombre brut de ventes.",
  "recipe_analyzer_tooltip_confidence": "Quantité d'historique de ventes sur 7 jours dont dispose le monde de vente pour cet objet : avec peu de ventes, une seule vente chanceuse fixe le prix.",
  "recipe_analyzer_tooltip_volume_30d": "Unités vendues sur le monde de vente au cours des 30 derniers jours. L'afficher charge un jeu de données sur 30 jours, une fois par monde.",
  "recipe_analyzer_tooltip_vwap_30d": "Prix moyen pondéré par les volumes sur le monde de vente au cours des 30 derniers jours, avec son écart en pourcentage par rapport au prix.",
 },
 "de": {
  "labs_analyzer_recipe_title": "Rezept-Analyse: das Marktmodell",
  "labs_analyzer_recipe_desc": "Ein Schalter für das ganze Werkzeug: die Gewinnformel als Steuerelement über der Tabelle, eine sortierbare Spalte je Preissignal mit „verwenden“-Schaltfläche, Sprunggewinn / Einheit und zu besuchende Welten sowie die Marktspalten — Gewinn/Tag, Trend, Drift, Volumen (30T) und VWAP (30T).",
  "analyzer_picker_group_market": "Markt",
  "analyzer_picker_group_location": "Ort",
  "recipe_analyzer_col_volume_30d": "Volumen (30T)",
  "recipe_analyzer_col_vwap_30d": "VWAP (30T)",
  "recipe_analyzer_col_drift": "Drift",
  "recipe_analyzer_window_7d": "7T",
  "analyzer_price_vs_median": "vs. Median {{pct}}",
  "recipe_analyzer_tooltip_profit_per_day": "Gewinn pro Einheit mal Verkäufe pro Tag der letzten 7 Tage auf der Verkaufswelt.",
  "recipe_analyzer_tooltip_trend": "Stündlicher Preis auf der Verkaufswelt über die letzten 7 Tage. Wird für die sichtbaren Zeilen geladen; die Stundenhistorie wird nicht nachgetragen, ein selten gehandelter Gegenstand kann daher leer bleiben.",
  "recipe_analyzer_tooltip_drift": "Preisänderung vom Anfang zum Ende dieses 7-Tage-Trends. Innerhalb von ±1 % gilt sie als unverändert.",
  "recipe_analyzer_tooltip_daily_sales": "Verkäufe pro Tag auf der Verkaufswelt über die letzten 7 Tage, aus der rohen Verkaufszahl.",
  "recipe_analyzer_tooltip_confidence": "Wie viel 7-Tage-Verkaufshistorie die Verkaufswelt für diesen Gegenstand hat: Bei wenigen Verkäufen bestimmt ein einzelner Glücksverkauf den Preis.",
  "recipe_analyzer_tooltip_volume_30d": "Auf der Verkaufswelt in den letzten 30 Tagen verkaufte Einheiten. Beim Einblenden wird einmal pro Welt ein eigener 30-Tage-Datensatz geladen.",
  "recipe_analyzer_tooltip_vwap_30d": "Volumengewichteter Durchschnittspreis auf der Verkaufswelt über die letzten 30 Tage, mit seiner Abweichung in Prozent gegenüber dem Preis.",
 },
 "ja": {
  "labs_analyzer_recipe_title": "レシピアナライザー：マーケットモデル",
  "labs_analyzer_recipe_desc": "ツール全体をこの1つの設定でまとめます：表の上に利益計算式の操作パネル、価格シグナルごとの並べ替え可能な列と「使う」ボタン、移動利益 / 個と訪問ワールド、そしてマーケット列（1日の利益・推移・価格推移・取引数量（30日）・VWAP（30日））。",
  "analyzer_picker_group_market": "マーケット",
  "analyzer_picker_group_location": "場所",
  "recipe_analyzer_col_volume_30d": "取引数量（30日）",
  "recipe_analyzer_col_vwap_30d": "VWAP（30日）",
  "recipe_analyzer_col_drift": "価格推移",
  "recipe_analyzer_window_7d": "7日",
  "analyzer_price_vs_median": "中央値比 {{pct}}",
  "recipe_analyzer_tooltip_profit_per_day": "1個あたりの利益 × 販売ワールドでの直近7日間の1日あたり販売数。",
  "recipe_analyzer_tooltip_trend": "販売ワールドでの直近7日間の1時間ごとの価格。表示中の行だけ取得します。時間別の履歴は遡って補完されないため、取引の少ないアイテムは空欄のままになることがあります。",
  "recipe_analyzer_tooltip_drift": "この7日間の推移の始点から終点までの価格変動。±1%以内は横ばいとして表示します。",
  "recipe_analyzer_tooltip_daily_sales": "販売ワールドでの直近7日間の1日あたり販売数（実際の販売件数から算出）。",
  "recipe_analyzer_tooltip_confidence": "このアイテムについて販売ワールドに7日間の販売履歴がどれだけあるか。件数が少ないと、1件の高値でも価格が決まってしまいます。",
  "recipe_analyzer_tooltip_volume_30d": "販売ワールドでの直近30日間の販売個数。表示するとワールドごとに1回、30日分のデータを読み込みます。",
  "recipe_analyzer_tooltip_vwap_30d": "販売ワールドでの直近30日間の出来高加重平均価格と、価格に対する変化率。",
 },
 "cn": {
  "labs_analyzer_recipe_title": "配方分析器：市场模型",
  "labs_analyzer_recipe_desc": "整个工具只用这一个开关：表格上方的利润公式操作面板、每个价格信号一个可排序列（含“使用”按钮）、跳服收益 / 单位与需前往的服务器，以及市场列——日利润、走势、价格走势、成交量（30天）和 VWAP（30天）。",
  "analyzer_picker_group_market": "市场",
  "analyzer_picker_group_location": "位置",
  "recipe_analyzer_col_volume_30d": "成交量（30天）",
  "recipe_analyzer_col_vwap_30d": "VWAP（30天）",
  "recipe_analyzer_col_drift": "价格走势",
  "recipe_analyzer_window_7d": "7天",
  "analyzer_price_vs_median": "对比中位 {{pct}}",
  "recipe_analyzer_tooltip_profit_per_day": "单件利润 × 售出服务器最近 7 天的日均成交笔数。",
  "recipe_analyzer_tooltip_trend": "售出服务器最近 7 天的每小时价格。只为可见的行加载；小时级历史不会回补，冷门物品可能一直为空。",
  "recipe_analyzer_tooltip_drift": "该 7 天走势从起点到终点的价格变化。±1% 以内视为持平。",
  "recipe_analyzer_tooltip_daily_sales": "售出服务器最近 7 天的日均成交笔数（按原始成交数计算）。",
  "recipe_analyzer_tooltip_confidence": "售出服务器上该物品 7 天成交历史的样本量：样本太少时，一笔运气好的成交就能决定价格。",
  "recipe_analyzer_tooltip_volume_30d": "售出服务器最近 30 天的成交件数。显示该列会按服务器加载一份单独的 30 天数据。",
  "recipe_analyzer_tooltip_vwap_30d": "售出服务器最近 30 天的成交量加权平均价，以及它相对价格的百分比。",
 },
 "ko": {
  "labs_analyzer_recipe_title": "제작 레시피 분석기: 시장 모델",
  "labs_analyzer_recipe_desc": "이 하나의 설정으로 도구 전체를 켭니다: 표 위의 이익 공식 조작 패널, 가격 신호마다 정렬 가능한 열과 “사용” 버튼, 이동 이득 / 개와 방문할 서버, 그리고 시장 열(일일 수익, 추세, 가격 추이, 거래량 (30일), VWAP (30일)).",
  "analyzer_picker_group_market": "시장",
  "analyzer_picker_group_location": "위치",
  "recipe_analyzer_col_volume_30d": "거래량 (30일)",
  "recipe_analyzer_col_vwap_30d": "VWAP (30일)",
  "recipe_analyzer_col_drift": "가격 추이",
  "recipe_analyzer_window_7d": "7일",
  "analyzer_price_vs_median": "중앙값 대비 {{pct}}",
  "recipe_analyzer_tooltip_profit_per_day": "개당 이익 × 판매 서버의 최근 7일 일일 판매 건수.",
  "recipe_analyzer_tooltip_trend": "판매 서버의 최근 7일 시간별 가격. 화면에 보이는 행만 불러옵니다. 시간별 기록은 소급해서 채우지 않으므로 거래가 드문 아이템은 계속 비어 있을 수 있습니다.",
  "recipe_analyzer_tooltip_drift": "이 7일 추세의 시작과 끝 사이의 가격 변화. ±1% 이내는 보합으로 표시합니다.",
  "recipe_analyzer_tooltip_daily_sales": "판매 서버의 최근 7일 일일 판매 건수(원본 판매 수 기준).",
  "recipe_analyzer_tooltip_confidence": "판매 서버에 이 아이템의 7일 판매 기록이 얼마나 있는지: 표본이 적으면 운 좋은 한 건이 가격을 정합니다.",
  "recipe_analyzer_tooltip_volume_30d": "판매 서버에서 최근 30일 동안 팔린 수량. 표시하면 서버마다 한 번씩 별도의 30일 데이터를 불러옵니다.",
  "recipe_analyzer_tooltip_vwap_30d": "판매 서버의 최근 30일 거래량 가중 평균 가격과 가격 대비 백분율.",
 },
 "tc": {
  "labs_analyzer_recipe_title": "配方分析器：市場模型",
  "labs_analyzer_recipe_desc": "整個工具只用這一個開關：表格上方的利潤公式操作面板、每個價格訊號一個可排序欄位（含「使用」按鈕）、跳服收益 / 單位與需前往的伺服器，以及市場欄位——日利潤、走勢、價格走勢、成交量（30天）與 VWAP（30天）。",
  "analyzer_picker_group_market": "市場",
  "analyzer_picker_group_location": "位置",
  "recipe_analyzer_col_volume_30d": "成交量（30天）",
  "recipe_analyzer_col_vwap_30d": "VWAP（30天）",
  "recipe_analyzer_col_drift": "價格走勢",
  "recipe_analyzer_window_7d": "7天",
  "analyzer_price_vs_median": "對比中位 {{pct}}",
  "recipe_analyzer_tooltip_profit_per_day": "單件利潤 × 售出伺服器最近 7 天的日均成交筆數。",
  "recipe_analyzer_tooltip_trend": "售出伺服器最近 7 天的每小時價格。只為可見的列載入；小時級歷史不會回補，冷門物品可能一直是空的。",
  "recipe_analyzer_tooltip_drift": "該 7 天走勢從起點到終點的價格變化。±1% 以內視為持平。",
  "recipe_analyzer_tooltip_daily_sales": "售出伺服器最近 7 天的日均成交筆數（依原始成交數計算）。",
  "recipe_analyzer_tooltip_confidence": "售出伺服器上該物品 7 天成交紀錄的樣本數：樣本太少時，一筆運氣好的成交就能決定價格。",
  "recipe_analyzer_tooltip_volume_30d": "售出伺服器最近 30 天的成交件數。顯示這個欄位會依伺服器載入一份獨立的 30 天資料。",
  "recipe_analyzer_tooltip_vwap_30d": "售出伺服器最近 30 天的成交量加權平均價，以及它相對價格的百分比。",
 },
}

# The two fr labels the new ones sit beside spell the window without the
# space the rest of the fr file uses ("7 j" in trends_window_*, price_basis_*
# and signal_short_*); normalise them so the Market group reads consistently.
NORMALISE = {"fr": {"recipe_analyzer_col_volume": "Volume (7 j)",
                    "recipe_analyzer_col_vwap": "VWAP (7 j)"}}

ROOT = "ultros-frontend/ultros-app/locales"
for locale, keys in KEYS.items():
    assert len(keys) == 16, locale
    path = os.path.join(ROOT, f"{locale}.json")
    with io.open(path, encoding="utf-8") as f:
        lines = f.read().split("\n")
    if any('"labs_analyzer_recipe_title"' in l for l in lines):
        sys.exit(f"{path}: keys already present")
    idx = next(i for i, l in enumerate(lines) if '"signal_short_sale_avg"' in l)
    indent = re.match(r"\s*", lines[idx]).group(0)
    new = [indent + json.dumps(k, ensure_ascii=False) + ": " + json.dumps(v, ensure_ascii=False) + "," for k, v in keys.items()]
    lines[idx + 1:idx + 1] = new
    lines = [l for l in lines if not any(f'"{d}":' in l for d in DELETE)]
    for k, v in NORMALISE.get(locale, {}).items():
        at = next(i for i, l in enumerate(lines) if f'"{k}":' in l)
        ind = re.match(r"\s*", lines[at]).group(0)
        lines[at] = ind + json.dumps(k, ensure_ascii=False) + ": " + json.dumps(v, ensure_ascii=False) + ","
    with io.open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines))
    with io.open(path, encoding="utf-8") as f:
        loaded = json.load(f)  # still valid JSON
    assert all(d not in loaded for d in DELETE), locale
    # Two columns that sit side by side must never carry the same label: fr
    # and de render `analyzer_col_drift` exactly like `analyzer_col_spark`,
    # which is why the recipe analyzer has its own Drift key.
    assert loaded["recipe_analyzer_col_drift"] != loaded["analyzer_col_spark"], locale
    print(locale, len(loaded))
```

- [ ] **Step 8: Verify every locale parses and has every key exactly once**

Run:

```bash
for l in en fr de ja cn ko tc; do python -c "import json; d=json.load(open('ultros-frontend/ultros-app/locales/$l.json', encoding='utf-8')); print('$l', len(d), d['analyzer_picker_group_market'], d['recipe_analyzer_col_drift'])"; done
grep -c '"recipe_analyzer_window_7d"' ultros-frontend/ultros-app/locales/*.json
grep -rn 'labs_analyzer_ledger\|labs_analyzer_signal_columns\|analyzer-ledger\|analyzer-signal-columns' ultros-frontend/ultros-app/ integration/ | grep -v '\.jules'
```

Expected: every locale prints `1790`, its translation of "Market", and a Drift label that differs from its `analyzer_col_spark` (the script asserts that too); every `grep -c` prints `1`; the third grep prints **exactly one line** — `global_state/labs.rs`'s `the_retired_analyzer_tokens_no_longer_parse`, the test that asserts those two strings are dead. Nothing else: no locale key, no Settings arm, no e2e route, no kit test fixture (Step 5's `sed` retired those six `lab:` strings) and no comment (Step 6 item 0). `{{pct}}` is spelled identically in all seven locales — leptos-i18n takes the union of variable names across locales, and a misspelling breaks the build at every `t_string!` call site for that key.

- [ ] **Step 9: Collapse the two e2e lab routes into one**

In `integration/runner.cjs`, replace the two `?labs=` recipe-analyzer entries in the assertions map (`:83-92`) with:

```js
  // One Labs toggle for the whole tool. With it on, the Profit header
  // carries an "after 5% tax" sub-label at every width; the strip row
  // itself is md+ only, and the mobile pass reads innerText, which drops
  // display:none content. The lab columns are md+ only too, so the only
  // cross-device assertions are the title and that sub-label; the sweep
  // still checks console errors and horizontal overflow.
  "/recipe-analyzer?world=Gilgamesh&labs=analyzer-recipe&cols=confidence,cost-sale-median,rev-sale-median,hop-gain,hop-worlds": {
    titleIncludes: "Recipe Analyzer",
    bodyIncludesAny: ["after 5% tax"],
  },
```

and the same two entries in `getRoutes()` (`:138-140`) with the one route string. Task 10 appends the five new `?cols=` tokens to it.

- [ ] **Step 10: Run the tests**

Run: `cargo test -p ultros-app --lib -- global_state::labs`
Expected: PASS, 4 tests (`labs_cookie_round_trips_known_tokens_only`, `the_retired_analyzer_tokens_no_longer_parse`, `every_lab_token_is_listed_once`, `the_experiment_list_stays_short`).

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer`
Expected: PASS, 44 tests (no test is added or removed by this task; `signal_columns_have_unique_ids_and_sort_tokens` now asserts `LAB_ANALYZER_RECIPE` on the same ten columns, and the row-clip fix's `the_grid_call_opts_into_a_sized_row_spacer` is untouched).

Run: `cargo test -p ultros-app --lib -- analyzer_kit`
Expected: PASS, 56 tests (cells 4, columns 5, enrichment 12, formula 10, grid 6, hop 4, needed 9, signals 5, strip 1).

A key missing from a non-default locale only warns (`cargo::warning=Missing key … in locale …`) and falls back to en, so a green build is not the seven-locale check — Step 8 is.

- [ ] **Step 11: Commit**

```bash
git add ultros-frontend/ultros-app/src/global_state/labs.rs ultros-frontend/ultros-app/src/routes/settings.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-app/src/analyzer_kit/ ultros-frontend/ultros-app/locales/ integration/runner.cjs
git commit -m "feat(labs): one analyzer-recipe toggle replaces the ledger and signal-columns flags, plus phase E2's i18n keys"
```

---
### Task 2: `analysis.rs` gains the shared rate, sign-class and first-to-last helpers; the flip finder's Drift class folds

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analysis.rs:296-360` (the block holding `MIN_VELOCITY_SPAN_DAYS`, `ROI_DISPLAY_CEILING`, `velocity_per_day`, `profit_per_day`, `price_drift_pct`) and its tests around `:780-880`
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs:2807-2829` (the Drift cell's three-arm match) and the `use crate::analysis::{…}` list at the top

**Interfaces:**
- Consumes: nothing new.
- Produces (all `pub` in `analysis.rs`):
  - `pub fn profit_per_day_from_rate(profit: i32, rate: f32) -> i32` — `profit × rate`, computed in f64 and truncated to i32 (a saturating float→int cast). Read by `profit_per_day` here and by the recipe's Profit/day cell and comparator (Task 7).
  - `pub const DELTA_DEAD_BAND_PCT: f32 = 1.0;` — the noise floor a signed percent must clear to be coloured. Read by the flip finder's Drift cell (this task), the recipe's Drift cell and the Price median tell (Tasks 7 and 9).
  - `pub fn signed_delta_class(pct: Option<f32>, dead_band: f32) -> &'static str` — `text-emerald-300` above `+dead_band`, `text-red-300` below `-dead_band`, the muted token otherwise (and for `None`).
  - `pub fn first_to_last_pct(first: u32, last: u32) -> Option<f32>` — the percent from the first to the last price of a sparkline window; `None` when `first == 0` (no trade at the start, so there is nothing to compare against). Read by the recipe's sparkline fetch (Task 8).

- [ ] **Step 1: Write the failing tests**

Add to `analysis.rs`'s `mod tests`, after `profit_per_day_zero_without_sale_history`:

```rust
    #[test]
    fn profit_per_day_from_rate_is_the_shared_form() {
        // The flip finder's buffer velocity and the recipe's rollup rate
        // feed the same arithmetic.
        assert_eq!(profit_per_day_from_rate(1_000, 2.5), 2_500);
        assert_eq!(profit_per_day_from_rate(1_000, 0.25), 250);
        assert_eq!(profit_per_day_from_rate(-300, 3.0), -900);
        assert_eq!(profit_per_day_from_rate(1_000, 0.0), 0);
        // Truncation, not rounding: 999 * 1.5 = 1498.5.
        assert_eq!(profit_per_day_from_rate(999, 1.5), 1_498);
        // A float -> int cast saturates rather than wrapping.
        assert_eq!(profit_per_day_from_rate(i32::MAX, 1_000.0), i32::MAX);
    }

    #[test]
    fn signed_delta_class_has_a_dead_band() {
        assert_eq!(signed_delta_class(Some(4.0), 1.0), "text-emerald-300");
        assert_eq!(signed_delta_class(Some(-4.0), 1.0), "text-red-300");
        // Inside the band, and exactly on it, read neutral.
        let muted = "text-[color:var(--color-text-muted)]";
        assert_eq!(signed_delta_class(Some(0.4), 1.0), muted);
        assert_eq!(signed_delta_class(Some(1.0), 1.0), muted);
        assert_eq!(signed_delta_class(Some(-1.0), 1.0), muted);
        assert_eq!(signed_delta_class(None, 1.0), muted);
        // A zero dead band colours any non-zero sign (the movers' rule).
        assert_eq!(signed_delta_class(Some(0.2), 0.0), "text-emerald-300");
        // NaN is neither above nor below: neutral, never a panic.
        assert_eq!(signed_delta_class(Some(f32::NAN), 1.0), muted);
    }

    /// `analyzer.rs`'s three Drift arms cut at ±1.0 with `text-emerald-300`
    /// / `text-red-300` / muted; the new const and fn must reproduce exactly
    /// those thresholds (`signed_delta_class_has_a_dead_band` passes `1.0`
    /// by hand and so cannot pin the const). The cell's *text* is unchanged
    /// by construction — the fold touches only the class, and `+{d:.0}%`
    /// and `{d:.0}%` are `{d:+.0}%` over the ranges the old arms guarded, a
    /// property of the `+` flag rather than of this code — so the byte
    /// identity of `/flip-finder` rides on `routes::analyzer`'s 69 existing
    /// tests plus manual check 9 in the PR body.
    #[test]
    fn signed_delta_class_reproduces_the_flip_finders_drift_arms() {
        for d in [1.4f32, 4.6, 12.5, 99.5, 100.4] {
            assert_eq!(signed_delta_class(Some(d), DELTA_DEAD_BAND_PCT), "text-emerald-300");
        }
        for d in [-1.4f32, -3.6, -50.0] {
            assert_eq!(signed_delta_class(Some(d), DELTA_DEAD_BAND_PCT), "text-red-300");
        }
        for d in [0.0f32, 0.9, -0.9] {
            assert_eq!(
                signed_delta_class(Some(d), DELTA_DEAD_BAND_PCT),
                "text-[color:var(--color-text-muted)]"
            );
        }
    }

    #[test]
    fn first_to_last_pct_needs_a_first_trade() {
        assert_eq!(first_to_last_pct(100, 150), Some(50.0));
        assert_eq!(first_to_last_pct(100, 50), Some(-50.0));
        assert_eq!(first_to_last_pct(100, 100), Some(0.0));
        // No trade in the window's first bucket: no percentage exists.
        assert_eq!(first_to_last_pct(0, 150), None);
        assert_eq!(first_to_last_pct(0, 0), None);
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- analysis`
Expected: compile error — `cannot find function profit_per_day_from_rate`, `signed_delta_class`, `first_to_last_pct`, `cannot find value DELTA_DEAD_BAND_PCT`.

- [ ] **Step 3: Add the helpers and make `profit_per_day` delegate**

In `analysis.rs`, replace `profit_per_day` (`:319-327`) with:

```rust
/// Expected gil per day from repeating one trade: per-trade profit times a
/// sales-per-day rate. Truncates (a float→int cast, which saturates rather
/// than wrapping). The rate's provenance is the caller's: the flip finder
/// passes [`velocity_per_day`] off its six-sale buffer, the recipe analyzer
/// passes the 7-day rollup's `num_sold / 7`.
pub fn profit_per_day_from_rate(profit: i32, rate: f32) -> i32 {
    (profit as f64 * rate as f64) as i32
}

/// Expected gil per day from flipping one item repeatedly: per-flip profit
/// times [`velocity_per_day`]. Items that sell faster than daily earn more
/// than one flip's profit per day; slow movers earn a fraction of it.
/// Returns 0 when there is no sale history to rate the item with.
pub fn profit_per_day(profit: i32, summary: &SaleSummary) -> i32 {
    velocity_per_day(summary)
        .map(|v| profit_per_day_from_rate(profit, v))
        .unwrap_or(0)
}
```

and add, after `price_drift_pct`:

```rust
/// The noise floor a signed percent must clear before it is coloured.
/// Origin: the flip finder's Drift cell, where ±1% inside a six-sale window
/// is noise wearing a percentage sign. Reused by the recipe analyzer's
/// Drift column and its Price "vs median" tell, which read the same kind of
/// small, sample-limited percentage.
pub const DELTA_DEAD_BAND_PCT: f32 = 1.0;

/// The colour class for a signed percentage: green above `+dead_band`, red
/// below `-dead_band`, muted inside the band and when there is no figure.
/// `dead_band` is the caller's noise floor (0.0 colours every non-zero
/// sign). A NaN falls through both comparisons and reads neutral.
pub fn signed_delta_class(pct: Option<f32>, dead_band: f32) -> &'static str {
    match pct {
        Some(p) if p > dead_band => "text-emerald-300",
        Some(p) if p < -dead_band => "text-red-300",
        _ => "text-[color:var(--color-text-muted)]",
    }
}

/// Percent change across a sparkline window, from its first traded price to
/// its last. The server sends the first and last *non-zero* points
/// (`arrayFilter(x -> x > 0, points)`, `ultros-clickhouse/src/queries.rs:158-167`),
/// so `first == 0` means nothing traded anywhere in the window: no baseline
/// exists, and 0 is not a price.
pub fn first_to_last_pct(first: u32, last: u32) -> Option<f32> {
    (first > 0).then(|| (last as f32 - first as f32) / first as f32 * 100.0)
}
```

- [ ] **Step 4: Fold the flip finder's Drift class**

In `routes/analyzer.rs`, the Drift cell's match (`:2809-2818`) loses its two sign arms:

```rust
                                    {move || visible_cols().contains(COL_DRIFT).then(|| {
                                        // +/- 1% is inside the noise floor of a 6-sale window,
                                        // so it renders neutral rather than green/red — the
                                        // dead band `signed_delta_class` was folded out of.
                                        let class = signed_delta_class(row_drift, DELTA_DEAD_BAND_PCT);
                                        let (text, title) = match row_drift {
                                            Some(d) => (format!("{d:+.0}%"), None),
                                            None => (
                                                "—".to_string(),
                                                Some(t_string!(i18n, analyzer_drift_unavailable).to_string()),
                                            ),
                                        };
                                        view! {
                                            <div
                                                role="cell"
                                                title=title
                                                class=format!("px-3 py-2 w-[88px] shrink-0 flex items-center justify-end font-mono tabular-nums {class}")
                                            >
                                                {text}
                                            </div>
                                        }
                                    })}
```

and the import list at the top of the file gains `DELTA_DEAD_BAND_PCT, signed_delta_class` (alphabetical inside `use crate::analysis::{…}`: `DELTA_DEAD_BAND_PCT` first, `signed_delta_class` between `sale_tax` and `sniper_clamp`).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ultros-app --lib -- analysis`
Expected: PASS, 46 tests (42 + 4). The three `profit_per_day_*` tests still pass unchanged — that is the proof the delegation is pure.

Run: `cargo test -p ultros-app --lib -- routes::analyzer`
Expected: PASS, 69 tests. `drift_sorts_undriftable_rows_last_in_both_directions` and the two `drift_floor_*` tests are untouched (they exercise the comparator and the filter, not the cell).

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/analysis.rs ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "refactor(analysis): profit_per_day_from_rate, signed_delta_class and first_to_last_pct; the flip finder's Drift class folds"
```

---
### Task 3: The kit's lazy layer — `Layer::Lazy`, `LazyFeed`, `LazyNever`, `Enrich`, the spark store, the late stats

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs:1-18` (imports), `:27-50` (`ColumnKind` ×5), `:52-62` (`PickerGroup` ×2), `:72-95` (`Layer`, `LazyFeed`, `Sortability`, `sortability_for`), `:97-107` (`CellCtx` ×2 fields), `:167-209` (two `heading` arms), `:270-280` (`sort_from_token`'s match), `:289-596` (fixtures and tests)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs:16-18` (imports), and a new `Enrich<V>` block above `CellValue`
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/enrichment.rs:1-20` (imports), `:71-101` (`Enrichment`'s impl block), and a new `SparkKey` / `SparkValue` / `SparkStore` block
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/signals.rs:1-14` (imports), `:38-49` (beside `StatsIndex`)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/grid.rs:166-224` (`header_cell`'s match gains `LazyNever` to the unsortable arm) and its six `CellCtx` literals (`:490, 532, 574, 764, 784, 813`)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs:259, :310` (the two `CellCtx` literals)
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:2784` (the `CellCtx` literal gains two `None`s until Task 8)

**Interfaces:**
- Consumes: `Enrichment<K, V>` and `Absorb` (`enrichment.rs:45-101`), `StatsIndex` (`signals.rs:40`), `ItemSaleStats`.
- Produces:
  - `pub enum LazyFeed { Sparklines { hours: u16 } }` with `pub fn hours(self) -> u16`.
  - `pub enum Layer { RowLocal, Computed, Bulk, Lazy(LazyFeed) }`.
  - `pub enum Sortability<M> { No, By(M), LazyNever }`, with `sortability_for` mapping every `Layer::Lazy` to `LazyNever` whatever the page asks for.
  - `ColumnKind::{ProfitPerDay, VolumeUnits30, Vwap30, Trend, DriftSpark}`.
  - `PickerGroup::{Market, Location}` (between `Travel` and `Other`) and their two `heading` arms.
  - `CellCtx.sparklines: Option<RwSignal<SparkStore>>`, `CellCtx.stats_30: Option<LateStats>`.
  - `pub enum Enrich<V> { Loading, Missing, Ready(V) }` with `map` and `is_loading` (`cells.rs`).
  - `pub type SparkKey = (i32, bool);`, `pub struct SparkValue { points: Vec<u32>, delta_pct: Option<f32> }` + `impl Absorb`, `pub type SparkStore = Enrichment<SparkKey, SparkValue>;`, `pub fn Enrichment::state(&self, key: &K) -> Enrich<&V>` (`enrichment.rs`).
  - `pub type LateStats = RwSignal<Option<Arc<StatsIndex>>>;`, `pub fn stat_row_either(index: &StatsIndex, item_id: i32, prefer_hq: bool) -> Option<&ItemSaleStats>` (`signals.rs`).

  Every one of these is dead until Task 4 (the render arms) or Task 7 (the table rows) — deliberate, per the Global Constraints. The task gate is the focused test run; the branch gate is Task 10's `check_ci.sh`.

- [ ] **Step 1: Write the failing tests**

In `columns.rs`'s `mod tests`, extend `sortability_follows_the_layer` and add one test:

```rust
    #[test]
    fn sortability_follows_the_layer() {
        assert_eq!(
            sortability_for(Layer::RowLocal, Some(Col::Profit)),
            Sortability::By(Col::Profit)
        );
        assert_eq!(
            sortability_for(Layer::Bulk, Some(Col::Profit)),
            Sortability::By(Col::Profit)
        );
        assert_eq!(
            sortability_for(Layer::Computed, None::<Col>),
            Sortability::No
        );
        // A lazy column never sorts, even when the page names a mode: the
        // visible window holds a fraction of the rows.
        let feed = Layer::Lazy(LazyFeed::Sparklines { hours: 168 });
        assert_eq!(sortability_for(feed, None::<Col>), Sortability::LazyNever);
        assert_eq!(
            sortability_for(feed, Some(Col::Profit)),
            Sortability::LazyNever
        );
        assert_eq!(LazyFeed::Sparklines { hours: 168 }.hours(), 168);
    }

    /// A `?sort=` token pointing at a lazy column resolves to nothing, so a
    /// bookmarked URL cannot sort by data most rows do not have.
    #[test]
    fn a_lazy_column_is_unreachable_from_a_sort_token() {
        // `P_TREND` deliberately carries `sort_id: "trend"`, so this reaches
        // the new `Sortability::No | Sortability::LazyNever => None` arm
        // rather than the "no column has that token" path.
        assert_eq!(sort_from_token(&PICKER, "trend"), None);
        assert!(PICKER
            .iter()
            .all(|c| !matches!(c.sort, Sortability::By(_)) || c.sort_id != "trend"));
    }
```

and rework the picker fixture so the two new groups are exercised: `P_CONF`'s group becomes `PickerGroup::Market`, and two entries join `PICKER` (which becomes `[ToolColumnMeta<(), Col>; 7]`):

```rust
    fn lbl_trend(_: I18nContext<Locale, I18nKeys>) -> String {
        "Trend".into()
    }
    fn lbl_world(_: I18nContext<Locale, I18nKeys>) -> String {
        "Listing world".into()
    }
    static P_TREND: ColumnSpec = ColumnSpec {
        kind: ColumnKind::Trend,
        label: lbl_trend,
        group: PickerGroup::Market,
    };
    static P_WORLD: ColumnSpec = ColumnSpec {
        kind: ColumnKind::ListingWorld,
        label: lbl_world,
        group: PickerGroup::Location,
    };
```

```rust
        ToolColumnMeta {
            spec: &P_TREND,
            id: "trend",
            // A token on an unsortable column: `sort_from_token` must still
            // refuse it (`a_lazy_column_is_unreachable_from_a_sort_token`).
            sort_id: "trend",
            sort: sortability_for(Layer::Lazy(LazyFeed::Sparklines { hours: 168 }), None),
            lab: Some("analyzer-recipe"),
            ..PBASE
        },
        ToolColumnMeta {
            spec: &P_WORLD,
            id: "listing-world",
            ..PBASE
        },
```

and `grouped_picker_keeps_option_order`'s assertions become:

```rust
            let ids: Vec<&str> = got.iter().map(|o| o.id).collect();
            assert_eq!(
                ids,
                [
                    "rev-sale-median",
                    "cost-listing-min",
                    "cost-sale-avg",
                    "hop-gain",
                    "confidence",
                    "trend",
                    "listing-world"
                ]
            );
```

with the heading assertions extended by two:

```rust
            assert_eq!(got[4].group.as_ref().unwrap().label, "Market");
            assert_eq!(got[5].group.as_ref().unwrap().label, "Market");
            assert_eq!(got[6].group.as_ref().unwrap().label, "Location");
            // The flat picker never lists a lab-gated column.
            let flat = picker_options(&PICKER, i18n);
            assert_eq!(
                flat.iter().map(|o| o.id).collect::<Vec<_>>(),
                ["confidence", "listing-world"]
            );
```

(the `got[3]` "Travel" assertion and the `got[0..2]` label/suffix assertions are unchanged; the old `got[4]` = "Other" assertion is replaced by the three above).

In `cells.rs`'s `mod tests`:

```rust
    #[test]
    fn enrich_maps_the_payload_and_keeps_the_state() {
        assert_eq!(Enrich::Ready(2u8).map(|v| v * 2), Enrich::Ready(4u8));
        assert_eq!(Enrich::<u8>::Missing.map(|v| v * 2), Enrich::Missing);
        assert_eq!(Enrich::<u8>::Loading.map(|v| v * 2), Enrich::Loading);
        assert!(Enrich::<u8>::Loading.is_loading());
        assert!(!Enrich::<u8>::Missing.is_loading());
        assert!(!Enrich::Ready(1u8).is_loading());
    }
```

In `enrichment.rs`'s `mod tests`:

```rust
    #[test]
    fn state_tells_loading_from_missing_from_ready() {
        let mut store: SparkStore = SparkStore::default();
        let key: SparkKey = (42, true);
        assert!(store.state(&key).is_loading());
        store.merge(
            &[key, (43, false)],
            vec![(
                key,
                SparkValue {
                    points: vec![1, 2, 3],
                    delta_pct: Some(200.0),
                },
            )],
        );
        // The key that came back: Ready, with its payload.
        match store.state(&key) {
            Enrich::Ready(v) => {
                assert_eq!(v.points, vec![1, 2, 3]);
                assert_eq!(v.delta_pct, Some(200.0));
            }
            other => panic!("{other:?}"),
        }
        // Requested, nothing came back: settled without a value.
        assert_eq!(store.state(&(43, false)), Enrich::Missing);
        // Never requested: still loading.
        assert!(store.state(&(44, false)).is_loading());
        // Absorb replaces: one indivisible feed per key.
        let mut v = SparkValue {
            points: vec![1],
            delta_pct: None,
        };
        v.absorb(SparkValue {
            points: vec![9, 9],
            delta_pct: Some(0.0),
        });
        assert_eq!(v.points, vec![9, 9]);
        assert_eq!(v.delta_pct, Some(0.0));
    }
```

In `signals.rs`'s `mod tests`:

```rust
    #[test]
    fn stat_row_either_falls_back_to_the_other_quality() {
        let mut index: StatsIndex = StatsIndex::new();
        index.insert((7, false), stat_row(7, false, 100));
        assert_eq!(stat_row_either(&index, 7, false).map(|r| r.min_price), Some(100));
        // HQ preferred but absent: the NQ row is what actually traded.
        assert_eq!(stat_row_either(&index, 7, true).map(|r| r.min_price), Some(100));
        index.insert((7, true), stat_row(7, true, 250));
        assert_eq!(stat_row_either(&index, 7, true).map(|r| r.min_price), Some(250));
        assert_eq!(stat_row_either(&index, 7, false).map(|r| r.min_price), Some(100));
        assert!(stat_row_either(&index, 8, false).is_none());
    }
```

`signals.rs`'s test module has no per-row fixture helper — its only builder is `fn stats(rows: &[(i32, bool, i32, i32, i32)]) -> BulkSaleStats` (`:150`), and `stat_only_has_no_fallback` spells its two rows out inline — so add one beside `stats` and leave that test's literals alone (they set `median_price` / `avg_price` / `num_sold`, which this helper does not):

```rust
    fn stat_row(item_id: i32, hq: bool, min_price: i32) -> ItemSaleStats {
        ItemSaleStats {
            item_id,
            hq,
            min_price,
            ..Default::default()
        }
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- analyzer_kit`
Expected: compile errors — `no variant Lazy on Layer`, `cannot find LazyFeed`, `no variant LazyNever`, `cannot find type SparkStore`, `no method state`, `cannot find function stat_row_either`, `cannot find Enrich`.

- [ ] **Step 3: The layer types in `columns.rs`**

Imports gain `use leptos::prelude::RwSignal;`, `use super::enrichment::SparkStore;` and `use super::signals::LateStats;` (`cells::CellValue` and `formula::PriceSignal` are already imported; the kit's modules already reference each other both ways).

`ColumnKind` gains five variants — `ProfitPerDay` after `Roi`, and the four market kinds after `Vwap7`:

```rust
    Roi,
    /// Profit times a sales-per-day rate. Computed, never fetched.
    ProfitPerDay,
```

```rust
    Vwap7,
    /// Units sold in a 30-day window (a different kind from the 7-day one:
    /// kinds name definitions, not labels).
    VolumeUnits30,
    /// Volume-weighted average price over a 30-day window.
    Vwap30,
    /// The hourly price series over a lazily fetched window.
    Trend,
    /// The first-to-last percent of that same series. Named for its
    /// definition: the spec's `DriftBuffer` is the recent-sales-buffer
    /// drift the flip finder shows, a different number from a different
    /// body.
    DriftSpark,
```

`PickerGroup` gains two variants before `Other`:

```rust
    Travel,
    /// Sale-history columns: confidence, last sold, volume, VWAP, tax,
    /// profit/day, trend, drift and the 30-day pair.
    Market,
    /// Where the cheapest listing is: world, datacenter.
    Location,
    /// The always-on columns. None of them has a `?cols=` token, so this
    /// group never reaches the picker; it exists because every
    /// `ColumnSpec` names one.
    Other,
```

`Layer`, `LazyFeed`, `Sortability` and `sortability_for` become:

```rust
/// A lazily fetched, visible-window feed. The window is part of the feed:
/// kinds name definitions, so a 168-hour sparkline and a 24-hour one are
/// the same feed with different windows, and a column declares which.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LazyFeed {
    /// `POST /api/v1/sparklines/{world}`: `hours` hourly VWAP points,
    /// oldest first, zeros for hours with no trade. The server clamps
    /// `hours` to [6, 168] and rejects more than 200 keys per request.
    Sparklines { hours: u16 },
}

impl LazyFeed {
    /// The feed's window, for the request the page builds — the reader
    /// that keeps `hours` from being a write-only field.
    pub fn hours(self) -> u16 {
        match self {
            LazyFeed::Sparklines { hours } => hours,
        }
    }
}

/// Where a column's value comes from. Sortability is derived from it:
/// anything complete for every row before the sorted memo runs sorts.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Layer {
    /// Present on the row as built.
    RowLocal,
    /// Derived from other row fields.
    Computed,
    /// From one whole-scope body fetched before the table renders.
    Bulk,
    /// Fetched per visible window after the table renders, so most rows
    /// have no value when the sorted memo runs.
    Lazy(LazyFeed),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Sortability<M> {
    No,
    By(M),
    /// A lazy column: never sortable, whatever the page asks for.
    LazyNever,
}

pub const fn sortability_for<M: Copy>(layer: Layer, wanted: Option<M>) -> Sortability<M> {
    match (layer, wanted) {
        (Layer::Lazy(_), _) => Sortability::LazyNever,
        (Layer::RowLocal | Layer::Computed | Layer::Bulk, Some(m)) => Sortability::By(m),
        (_, None) => Sortability::No,
    }
}
```

`CellCtx` gains the two handles:

```rust
/// Per-render context a cell extractor may read. The two signal handles let
/// a `fn`-pointer extractor reach page-level lazy data without the table
/// giving up its `static` column list; they are read inside the row's
/// reactive closure, so a merge re-renders the mounted rows.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CellCtx {
    pub now_unix: i64,
    /// The page's Labs toggle (`analyzer-recipe` on the recipe analyzer):
    /// the Price slot renders its note sub-line only under it.
    pub preview: bool,
    /// Cost signals the sub-craft cap left unpriced, by
    /// `PriceSignal::index`; their cells render "—" with the cap title.
    pub capped_cost: [bool; 4],
    /// The page's visible-window sparkline store. `None` on a page without
    /// one (and in tests): the cell then renders its loading shape, which
    /// is what the server renders too.
    pub sparklines: Option<RwSignal<SparkStore>>,
    /// The page's client-only 30-day statistics body. `None` on a page
    /// without one; `Some(signal holding None)` while it is in flight.
    pub stats_30: Option<LateStats>,
}
```

and `sort_from_token`'s match gains the arm that makes a lazy column unreachable from a URL:

```rust
        .and_then(|c| match c.sort {
            Sortability::By(m) => Some(m),
            Sortability::No | Sortability::LazyNever => None,
        })
```

- [ ] **Step 4: The two `heading` arms**

In `heading`, before the `Other` arm:

```rust
        PickerGroup::Market => PickerHeading {
            label: t_string!(i18n, analyzer_picker_group_market).to_string(),
            title: None,
        },
        PickerGroup::Location => PickerHeading {
            label: t_string!(i18n, analyzer_picker_group_location).to_string(),
            title: None,
        },
```

- [ ] **Step 5: `Enrich<V>` in `cells.rs`**

Above `CellValue`:

```rust
/// The three states a resource-backed cell can be in: the fetch has not
/// answered for this key yet, it answered with nothing, or it answered.
/// `Missing` and `Ready` are settled — only `Loading` shimmers, and it is
/// what the server and the first client paint always render (the stores are
/// empty on both sides), which is what keeps hydration honest.
#[derive(Clone, Debug, PartialEq)]
pub enum Enrich<V> {
    Loading,
    Missing,
    Ready(V),
}

impl<V> Enrich<V> {
    /// Map the payload, keeping the state. Turns the borrowed
    /// `Enrich<&V>` a store read yields into the owned value a cell holds.
    pub fn map<U>(self, f: impl FnOnce(V) -> U) -> Enrich<U> {
        match self {
            Enrich::Loading => Enrich::Loading,
            Enrich::Missing => Enrich::Missing,
            Enrich::Ready(v) => Enrich::Ready(f(v)),
        }
    }

    /// Whether the cell shows its skeleton — the one state difference the
    /// one-shape rule lets a cell branch a class on.
    pub fn is_loading(&self) -> bool {
        matches!(self, Enrich::Loading)
    }
}
```

- [ ] **Step 6: The spark store and `Enrichment::state`**

In `enrichment.rs`, `use super::cells::Enrich;` joins the imports, `Enrichment`'s impl block gains

```rust
    /// The key's cell state: `Ready` with the value, `Missing` once a fetch
    /// has settled the key without one, `Loading` until then. The
    /// three-way read every lazy cell makes, in one place, so no page
    /// re-derives it from `get` and `is_settled`.
    pub fn state(&self, key: &K) -> Enrich<&V> {
        match (self.map.get(key), self.settled.contains(key)) {
            (Some(v), _) => Enrich::Ready(v),
            (None, true) => Enrich::Missing,
            (None, false) => Enrich::Loading,
        }
    }
```

and the module gains, after `Enrichment`'s impl:

```rust
/// A sparkline key: `(item_id, the quality the row's statistics resolved
/// to)`. The recipe analyzer's Trend and Drift columns share it — one feed,
/// two columns, one request.
pub type SparkKey = (i32, bool);

/// One key's hourly price series plus the percent from its first traded
/// price to its last, which colours the sparkline and is what the Drift
/// column shows. A `Vec<u32>` rather than an `Arc<[u32]>`: the `Sparkline`
/// component takes a `Vec` and would copy either way.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SparkValue {
    pub points: Vec<u32>,
    /// `None` when nothing traded anywhere in the window — `first_price`
    /// is the first *non-zero* point, so it is 0 only for an all-empty
    /// series (`analysis::first_to_last_pct`).
    pub delta_pct: Option<f32>,
}

impl Absorb for SparkValue {
    /// One indivisible feed: a key fetched again replaces.
    fn absorb(&mut self, newer: Self) {
        *self = newer;
    }
}

pub type SparkStore = Enrichment<SparkKey, SparkValue>;
```

- [ ] **Step 7: `LateStats` and `stat_row_either`**

In `signals.rs`, beside `StatsIndex` (`:38-49`), with `use leptos::prelude::RwSignal;` added to the imports:

```rust
/// A client-only sale-statistics body, filled by a page `Effect` after the
/// table has rendered: `None` on the server and on the first client paint,
/// `Some(index)` once it lands — an *empty* index if the fetch failed, so
/// cells settle to "—" instead of shimmering forever.
pub type LateStats = RwSignal<Option<Arc<StatsIndex>>>;

/// The statistics row for `(item, quality)`, preferring `prefer_hq` and
/// falling back to the other quality: the rule the pricing pass applies to
/// the 7-day body, so a row's 30-day figures come from the same quality its
/// 7-day ones did.
pub fn stat_row_either(
    index: &StatsIndex,
    item_id: i32,
    prefer_hq: bool,
) -> Option<&ItemSaleStats> {
    index
        .get(&(item_id, prefer_hq))
        .or_else(|| index.get(&(item_id, !prefer_hq)))
}
```

- [ ] **Step 8: Keep every `CellCtx` literal compiling**

`header_cell`'s unsortable arm takes the new variant with no markup change (Task 5 gives it extras):

```rust
        // Unsortable headers were `t!(..)` on the page (locale-reactive);
        // keep that by resolving the label inside a closure. A lazy column
        // is unsortable for a different reason and renders the same way.
        (Sortability::No | Sortability::LazyNever, _) => view! {
            <div role="columnheader" class=class>{move || label_fn(i18n)}</div>
        }
        .into_any(),
```

and **every** `CellCtx` literal in the crate gains `sparklines: None, stats_30: None` — the same ten sites Task 1 Step 5 renamed, found the same way (`grep -rn 'CellCtx {' ultros-frontend/ultros-app/src/`, which also matches the struct definition at `columns.rs:99` — skip that one): `columns.rs:438`, `cells.rs:259` and `:310`, `grid.rs:490, 532, 574, 764, 784, 813` (the pair inside `row_min_width_reaches_the_scrollers_spacer` included) and `recipe_analyzer.rs:2784`, whose real handles arrive in Task 8. Nine of the ten are single-line `Signal::derive(|| CellCtx { now_unix: 0, preview: false, capped_cost: [false; 4] })` forms that gain `, sparklines: None, stats_30: None` before the closing brace; `columns.rs:438`, `cells.rs`'s two and the recipe page's are multi-line and take the two fields as their own lines. `CellCtx` derives no `Default`, so a missed literal is a hard `missing fields` error naming the file and line — noisy, never silent.

- [ ] **Step 9: Run the tests**

Run: `cargo test -p ultros-app --lib -- analyzer_kit`
Expected: PASS, 60 tests (56 plus `a_lazy_column_is_unreachable_from_a_sort_token`, `enrich_maps_the_payload_and_keeps_the_state`, `state_tells_loading_from_missing_from_ready` and `stat_row_either_falls_back_to_the_other_quality`; `sortability_follows_the_layer` and `grouped_picker_keeps_option_order` grew but did not multiply). Dead-code warnings on `LazyFeed::hours`, `SparkValue`, `SparkStore`, `Enrich`, `LateStats`, `stat_row_either` and the five `ColumnKind` variants are expected until Tasks 4 and 7.

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer`
Expected: PASS, 44 tests (the `CellCtx` literal is the only change).

- [ ] **Step 10: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/ ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(analyzer-kit): the lazy layer — Layer::Lazy, LazyFeed, Sortability::LazyNever, Enrich, SparkStore, LateStats"
```

---
### Task 4: Four lazy/late cells and the Price median tell, all in `render_cell`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs:1-18` (imports), `:19-64` (`CellValue` ×4, `CellNote::VsMedian`), `:86` (`SUB_LINE_GEOM`, `SKELETON_BAR`, `bar_class`), `:114-235` (`render_cell`: the `GilWithNote` arm rewritten, four arms added), `:237-421` (tests)

**Interfaces:**
- Consumes: `Enrich<V>` and `SparkValue` (Task 3), `analysis::{DELTA_DEAD_BAND_PCT, signed_delta_class}` (Task 2), `components::sparkline::Sparkline`, `components::gil::{Gil, GilOrDash}`, the i18n keys `analyzer_drift_unavailable`, `analyzer_price_listing_fallback`, `analyzer_price_vs_median` (Task 1).
- Produces:
  - `CellValue::Sparkline(Enrich<SparkValue>)`, `CellValue::LazyPct(Enrich<Option<f32>>)`, `CellValue::LateCount(Enrich<u64>)`, `CellValue::LateGilWithPct(Enrich<(i32, Option<f32>)>)` and their `render_cell` arms.
  - `CellNote::VsMedian { listing: bool, pct: f32 }`. **`CellNote` loses its `Eq` derive** (an `f32` field): nothing requires it — `CellValue` derives only `Clone, Debug, PartialEq`.
  - Private `SKELETON_BAR`, `SUB_LINE_GEOM`, `fn bar_class(loading: bool) -> &'static str`.

- [ ] **Step 1: Write the failing tests**

Add to `cells.rs`'s `mod tests`:

```rust
    /// Every lazy or late cell renders the same elements in every state:
    /// the skeleton bar and the value slot are both always present and swap
    /// by class. The one exception, and why it is safe: the Trend cell's
    /// `Ready` adds the `<svg>` the `Sparkline` component draws *inside*
    /// its fixed span. `Loading` and `Missing` are shaped alike, and
    /// `Loading` is what the server and the first client paint both render
    /// (the stores are empty on both sides), so hydration never sees
    /// `Ready`.
    #[test]
    fn lazy_cells_keep_one_shape_per_variant() {
        use crate::analyzer_kit::enrichment::SparkValue;
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let ctx = CellCtx {
                now_unix: 1_700_000_000,
                preview: true,
                capped_cost: [false; 4],
                sparklines: None,
                stats_30: None,
            };
            let render = |v: CellValue| render_cell("w-28", v, i18n, &ctx).unwrap().to_html();

            let spark = |e| render(CellValue::Sparkline(e));
            let loading = spark(Enrich::Loading);
            let missing = spark(Enrich::Missing);
            let ready = spark(Enrich::Ready(SparkValue {
                points: vec![100, 110, 120],
                delta_pct: Some(20.0),
            }));
            assert_eq!(count(&loading, "<div"), count(&missing, "<div"));
            assert_eq!(count(&loading, "<span"), count(&missing, "<span"));
            assert_eq!(count(&loading, "role=\"cell\""), 1);
            assert_eq!(count(&ready, "role=\"cell\""), 1);
            assert!(loading.contains("skeleton-shimmer"), "{loading}");
            assert!(!missing.contains("skeleton-shimmer"), "{missing}");
            assert!(ready.contains("<svg"), "{ready}");
            assert!(!loading.contains("<svg") && !missing.contains("<svg"));

            let pct = |e| render(CellValue::LazyPct(e));
            let p_loading = pct(Enrich::Loading);
            let p_missing = pct(Enrich::Missing);
            let p_up = pct(Enrich::Ready(Some(4.0)));
            let p_down = pct(Enrich::Ready(Some(-4.0)));
            let p_flat = pct(Enrich::Ready(Some(0.4)));
            let p_none = pct(Enrich::Ready(None));
            for h in [&p_missing, &p_up, &p_down, &p_flat, &p_none] {
                assert_eq!(count(&p_loading, "<div"), count(h, "<div"), "{p_loading}\n{h}");
                assert_eq!(count(&p_loading, "<span"), count(h, "<span"));
            }
            assert!(p_up.contains("+4%") && p_up.contains("text-emerald-300"), "{p_up}");
            assert!(p_down.contains("-4%") && p_down.contains("text-red-300"), "{p_down}");
            assert!(p_flat.contains("+0%") && !p_flat.contains("emerald"), "{p_flat}");
            // Settled with no percentage reads like no data, with the tell.
            assert!(p_none.contains("—") && p_none.contains("Not enough sales"), "{p_none}");
            assert!(p_missing.contains("—"), "{p_missing}");

            let cnt = |e| render(CellValue::LateCount(e));
            let c_loading = cnt(Enrich::Loading);
            let c_missing = cnt(Enrich::Missing);
            let c_ready = cnt(Enrich::Ready(1_234u64));
            for h in [&c_missing, &c_ready] {
                assert_eq!(count(&c_loading, "<div"), count(h, "<div"));
                assert_eq!(count(&c_loading, "<span"), count(h, "<span"));
            }
            assert!(c_ready.contains("1234"), "{c_ready}");
            assert!(c_missing.contains("—"), "{c_missing}");

            let gil = |e| render(CellValue::LateGilWithPct(e));
            let g_loading = gil(Enrich::Loading);
            let g_missing = gil(Enrich::Missing);
            let g_ready = gil(Enrich::Ready((820, Some(-6.0))));
            let g_zero = gil(Enrich::Ready((0, None)));
            for h in [&g_missing, &g_ready, &g_zero] {
                assert_eq!(count(&g_loading, "<div"), count(h, "<div"), "{g_loading}\n{h}");
                assert_eq!(count(&g_loading, "<span"), count(h, "<span"));
            }
            assert!(g_ready.contains("-6%"), "{g_ready}");
            assert!(g_missing.contains("—") && g_zero.contains("—"));
        });
    }

    /// The Price note line keeps Phase D's exact class and text until the
    /// median tell is in it, and the tell's colour composes back to that
    /// same class inside the dead band.
    #[test]
    fn the_price_note_adds_the_median_tell_without_moving_phase_d() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let ctx = CellCtx {
                now_unix: 0,
                preview: true,
                capped_cost: [false; 4],
                sparklines: None,
                stats_30: None,
            };
            let render = |note| {
                render_cell("w-32", CellValue::GilWithNote { amount: 120, note }, i18n, &ctx)
                    .unwrap()
                    .to_html()
            };
            let plain = render(CellNote::None);
            let listing = render(CellNote::ListingFallback);
            let up = render(CellNote::VsMedian { listing: false, pct: 4.0 });
            let both = render(CellNote::VsMedian { listing: true, pct: -4.0 });
            let flat = render(CellNote::VsMedian { listing: false, pct: 0.4 });
            for h in [&listing, &up, &both, &flat] {
                assert_eq!(count(&plain, "<div"), count(h, "<div"), "{plain}\n{h}");
            }
            // Phase D's two notes are byte-for-byte what they were.
            let sub = format!("class=\"{SUB_LINE}\"");
            assert!(plain.contains(&sub), "{plain}");
            assert!(listing.contains(&sub) && listing.contains(">listing<"), "{listing}");
            assert!(up.contains("vs median +4%") && up.contains("text-emerald-300"), "{up}");
            assert!(both.contains("listing · vs median -4%") && both.contains("text-red-300"), "{both}");
            // Inside the dead band the composed class IS the plain one.
            assert!(flat.contains(&sub), "{flat}");
            assert_eq!(
                format!(
                    "{SUB_LINE_GEOM} {}",
                    crate::analysis::signed_delta_class(None, crate::analysis::DELTA_DEAD_BAND_PCT)
                ),
                SUB_LINE
            );
        });
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::cells`
Expected: compile error — `no variant Sparkline / LazyPct / LateCount / LateGilWithPct on CellValue`, `no variant VsMedian on CellNote`, `cannot find value SUB_LINE_GEOM`.

- [ ] **Step 3: The variants**

`cells.rs`'s imports gain `use crate::analysis::{DELTA_DEAD_BAND_PCT, roi_badge_class, signed_delta_class};`, `use crate::components::sparkline::Sparkline;` and `use super::enrichment::SparkValue;`. `CellValue` gains four variants (after `GilWithNote`, before `Hop`):

```rust
    /// A lazily fetched hourly price series, coloured by its own
    /// first-to-last percent.
    Sparkline(Enrich<SparkValue>),
    /// A lazily fetched signed percent (Drift). `Ready(None)` means the
    /// series had no first trade, so no percentage exists — it reads like
    /// `Missing`, with the same "not enough sales" tell.
    LazyPct(Enrich<Option<f32>>),
    /// A count from a body that lands after the table (Volume 30d).
    LateCount(Enrich<u64>),
    /// A gil amount and its percent against Price, from a body that lands
    /// after the table (VWAP 30d).
    LateGilWithPct(Enrich<(i32, Option<f32>)>),
```

and `CellNote` gains its third state, losing `Eq` (an `f32` field; nothing needs it):

```rust
/// The sub-line under a [`CellValue::GilWithNote`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum CellNote {
    None,
    /// The price fell back to a listing (the selected signal had no row on
    /// the sell world, or the sell world had no listing at all).
    ListingFallback,
    /// This price against the sell world's 7-day sale median, signed and
    /// coloured; `listing` keeps the fallback tell in front of it, so the
    /// line reads `listing · vs median +4%`.
    VsMedian { listing: bool, pct: f32 },
}
```

- [ ] **Step 4: The shared bits**

Beside `SUB_LINE` (`:86`):

```rust
const SUB_LINE: &str = "text-[10px] leading-3 text-[color:var(--color-text-muted)]";
/// The geometry half of [`SUB_LINE`], so a coloured sub-line can compose it
/// with `signed_delta_class`. Inside the dead band that composition is
/// `SUB_LINE` character for character, which is what keeps the Price note
/// identical for the states that predate the median tell.
const SUB_LINE_GEOM: &str = "text-[10px] leading-3";

/// The bar a lazy or late cell shows while its fetch is in flight. Inline
/// rather than `SingleLineSkeleton`: one shape needs the element present in
/// every state, and that component's `sr-only` "Loading…" would then be
/// announced on settled rows.
const SKELETON_BAR: &str = "skeleton-block skeleton-shimmer w-full h-3 rounded-md";

fn bar_class(loading: bool) -> &'static str {
    if loading { SKELETON_BAR } else { "hidden" }
}
```

- [ ] **Step 5: The `render_cell` arms**

Replace the `GilWithNote` arm and add the four new ones:

```rust
        CellValue::GilWithNote { amount, note } => {
            let (text, note_class) = match note {
                CellNote::None => (String::new(), SUB_LINE.to_string()),
                CellNote::ListingFallback => (
                    t_string!(i18n, analyzer_price_listing_fallback).to_string(),
                    SUB_LINE.to_string(),
                ),
                CellNote::VsMedian { listing, pct } => {
                    let tell = t_string!(
                        i18n,
                        analyzer_price_vs_median,
                        pct = format!("{pct:+.0}%")
                    )
                    .to_string();
                    let text = if listing {
                        format!(
                            "{} · {}",
                            t_string!(i18n, analyzer_price_listing_fallback),
                            tell
                        )
                    } else {
                        tell
                    };
                    (
                        text,
                        format!(
                            "{SUB_LINE_GEOM} {}",
                            signed_delta_class(Some(pct), DELTA_DEAD_BAND_PCT)
                        ),
                    )
                }
            };
            view! {
                <div role="cell" class=class>
                    <Gil amount=amount />
                    <div class=note_class>{text}</div>
                </div>
            }
            .into_any()
        }
        CellValue::Sparkline(state) => {
            let loading = state.is_loading();
            let (points, pct) = match state {
                Enrich::Ready(v) => (v.points, v.delta_pct.unwrap_or(0.0)),
                _ => (Vec::new(), 0.0),
            };
            view! {
                <div role="cell" class=class>
                    <div class=bar_class(loading) aria-hidden="true"></div>
                    <span class=if loading { "hidden" } else { "" }>
                        <Sparkline points=points pct_change=pct />
                    </span>
                </div>
            }
            .into_any()
        }
        CellValue::LazyPct(state) => {
            let loading = state.is_loading();
            let pct = match state {
                Enrich::Ready(p) => p,
                _ => None,
            };
            let (text, title) = match (loading, pct) {
                (true, _) => (String::new(), None),
                (false, Some(p)) => (format!("{p:+.0}%"), None),
                (false, None) => (
                    "—".to_string(),
                    Some(t_string!(i18n, analyzer_drift_unavailable).to_string()),
                ),
            };
            let colour = signed_delta_class(pct, DELTA_DEAD_BAND_PCT);
            view! {
                <div role="cell" class=class title=title>
                    <div class=bar_class(loading) aria-hidden="true"></div>
                    <span class=if loading { "hidden" } else { colour }>{text}</span>
                </div>
            }
            .into_any()
        }
        CellValue::LateCount(state) => {
            let loading = state.is_loading();
            let text = match state {
                Enrich::Ready(n) => n.to_string(),
                Enrich::Missing => "—".to_string(),
                Enrich::Loading => String::new(),
            };
            view! {
                <div role="cell" class=class>
                    <div class=bar_class(loading) aria-hidden="true"></div>
                    <span class=if loading { "hidden" } else { "" }>{text}</span>
                </div>
            }
            .into_any()
        }
        CellValue::LateGilWithPct(state) => {
            let loading = state.is_loading();
            let (amount, sub) = match state {
                Enrich::Ready((amount, pct)) => (
                    (amount > 0).then_some(amount),
                    pct.filter(|_| amount > 0)
                        .map(|p| format!("{p:+.0}%"))
                        .unwrap_or_default(),
                ),
                _ => (None, String::new()),
            };
            view! {
                <div role="cell" class=class>
                    <div class=bar_class(loading) aria-hidden="true"></div>
                    <div class=if loading { "hidden" } else { "" }>
                        <GilOrDash amount=amount />
                        <div class="text-xs text-[color:var(--color-text-muted)]">{sub}</div>
                    </div>
                </div>
            }
            .into_any()
        }
```

Note what the arms do **not** do: no arm swaps one element for another, none renders an `Option` child (which would write a `<!>` marker), and none reads a signal — the extractor has already sampled the store, so the value handed to `render_cell` is plain data.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::cells`
Expected: PASS, 7 tests (5 after Task 3, plus `lazy_cells_keep_one_shape_per_variant` and `the_price_note_adds_the_median_tell_without_moving_phase_d`). `render_cell_keeps_one_shape_per_variant` and `new_cells_keep_one_shape_per_variant` still pass untouched — the `GilWithNote` rewrite is proven byte-identical for the two notes that already existed.

Run: `cargo test -p ultros-app --lib -- analyzer_kit`
Expected: PASS, 62 tests (60 plus the two added here).

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/cells.rs
git commit -m "feat(analyzer-kit): Sparkline, LazyPct, LateCount and LateGilWithPct cells, and the Price vs-median note"
```

---
### Task 5: The grid — a `visible_range` prop, sub-labels without a pill, titles on unsortable headers

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/grid.rs:62-83` (`HeaderLine2.pill`, `HeaderExtra.header_class`, a sub-line class const), `:154-224` (`header_cell`), `:234-270` (the `visible_range` prop), `:301-343` (forwarding it to the scroller), `:594-765` (tests)
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:2140-2190` (the four `HeaderExtra` literals gain `pill: Some(..)` and `header_class: None`)

**Interfaces:**
- Consumes: `VirtualScroller`'s `#[prop(optional, into)] visible_range: Option<RwSignal<(usize, usize)>>` (`virtual_scroller.rs:171-175`), `SortableHeaderCell`'s `sub_label` / `trailing` / `title` props (`sort_header.rs:191-227`).
- Produces:
  - `HeaderLine2.pill: Option<HeaderPill>` — `None` is a sub-label with no button.
  - `HeaderExtra.header_class: Option<&'static str>` — the classes to use *while this extra is in effect*, so a column can become two-line only under the lab.
  - `AnalyzerGrid`'s `#[prop(optional)] visible_range: Option<RwSignal<(usize, usize)>>`.
  - `header_cell` arms for an unsortable column with a title and an optional second line.

- [ ] **Step 1: Write the failing tests**

In `grid.rs`'s `mod tests`, extend `header_extras_render_title_sub_label_and_pill`'s `extras` builder to `pill: Some(HeaderPill { aria: …, pressed })` and `header_class: None`, then add two tests:

```rust
    /// Line 2 without a pill: the sub-label renders, no button appears, and
    /// the extra's `header_class` replaces the column's while it is in
    /// effect (Daily sales and Confidence become two-line only under the
    /// lab; their flag-off classes must not move).
    #[test]
    fn a_second_line_without_a_pill_renders_no_button() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let mut by_kind = HashMap::new();
            by_kind.insert(
                SIGNAL_COL.spec.kind,
                HeaderExtra {
                    title: "Sales per day over 7 days".into(),
                    line2: Some(HeaderLine2 {
                        sub_label: "7d · Gilgamesh".into(),
                        pill: None,
                    }),
                    header_class: Some("w-28 px-4 py-2 leading-tight hidden md:flex"),
                },
            );
            let extras = Signal::derive(move || HeaderExtras {
                by_kind: by_kind.clone(),
            });
            let html = header_cell(
                &SIGNAL_COL,
                Signal::derive(|| None::<Col>),
                Signal::derive(|| None::<SortDir>),
                i18n,
                None,
                Some(extras),
                None,
            )
            .to_html();
            assert!(html.contains("title=\"Sales per day over 7 days\""), "{html}");
            assert!(html.contains("7d · Gilgamesh"), "{html}");
            assert!(!html.contains("<button"), "{html}");
            assert!(html.contains("w-28 px-4 py-2 leading-tight hidden md:flex"), "{html}");
        });
    }

    /// An unsortable header renders exactly today's markup with no extra,
    /// and gains a title (and a second line) when the page gives it one.
    #[test]
    fn unsortable_headers_take_a_title_and_a_second_line() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let none = Signal::derive(|| None::<Col>);
            let none_dir = Signal::derive(|| None::<SortDir>);
            // COLS[0] is the unsortable Item column.
            let plain = header_cell(&COLS[0], none, none_dir, i18n, None, None, None).to_html();
            assert!(plain.contains("role=\"columnheader\""), "{plain}");
            assert!(!plain.contains("title=") && !plain.contains("<span"), "{plain}");
            let empty = header_cell(
                &COLS[0],
                none,
                none_dir,
                i18n,
                None,
                Some(Signal::derive(HeaderExtras::default)),
                None,
            )
            .to_html();
            assert_eq!(empty, plain, "an empty extras map is the flag-off path");

            let with_line2 = |line2| {
                let mut by_kind = HashMap::new();
                by_kind.insert(
                    COLS[0].spec.kind,
                    HeaderExtra {
                        title: "Hourly price, last 7 days".into(),
                        line2,
                        header_class: None,
                    },
                );
                let extras = Signal::derive(move || HeaderExtras {
                    by_kind: by_kind.clone(),
                });
                header_cell(&COLS[0], none, none_dir, i18n, None, Some(extras), None).to_html()
            };
            let titled = with_line2(None);
            assert!(titled.contains("title=\"Hourly price, last 7 days\""), "{titled}");
            assert!(!titled.contains("<span"), "{titled}");
            let two_line = with_line2(Some(HeaderLine2 {
                sub_label: "7d · Gilgamesh".into(),
                pill: None,
            }));
            assert!(two_line.contains("title=\"Hourly price, last 7 days\""), "{two_line}");
            assert!(two_line.contains("7d · Gilgamesh"), "{two_line}");
            assert_eq!(two_line.matches("role=\"columnheader\"").count(), 1, "{two_line}");
        });
    }
```

and one for the new prop, beside `grid_renders_visible_columns_only`:

```rust
    /// The page's range signal reaches the scroller and changes no markup:
    /// the scroller writes it from a client `Effect`, which never runs on
    /// the server.
    #[test]
    fn visible_range_is_optional_and_changes_no_markup() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let range = RwSignal::new((0usize, 0usize));
            let render = |range: Option<RwSignal<(usize, usize)>>| {
                match range {
                    Some(range) => view! {
                        <AnalyzerGrid
                            columns=&COLS
                            rows=Signal::derive(|| vec![(0usize, Row(7))])
                            visible_cols=Signal::derive(HashSet::new)
                            sort_mode=Signal::derive(|| None::<Col>)
                            sort_dir=Signal::derive(|| None::<SortDir>)
                            ctx=Signal::derive(|| CellCtx { now_unix: 0, preview: false, capped_cost: [false; 4], sparklines: None, stats_30: None })
                            custom=Arc::new(|_: &Row, _: ColumnKind, class: &'static str| view! { <div role="cell" class=class>"x"</div> }.into_any())
                            layout=GridLayout { viewport_height: 720.0, row_height: 60.0, header_height: 64.0, overscan: 8 }
                            header_class="thead"
                            row_class=stripe
                            visible_range=range
                        />
                    }
                    .to_html(),
                    None => view! {
                        <AnalyzerGrid
                            columns=&COLS
                            rows=Signal::derive(|| vec![(0usize, Row(7))])
                            visible_cols=Signal::derive(HashSet::new)
                            sort_mode=Signal::derive(|| None::<Col>)
                            sort_dir=Signal::derive(|| None::<SortDir>)
                            ctx=Signal::derive(|| CellCtx { now_unix: 0, preview: false, capped_cost: [false; 4], sparklines: None, stats_30: None })
                            custom=Arc::new(|_: &Row, _: ColumnKind, class: &'static str| view! { <div role="cell" class=class>"x"</div> }.into_any())
                            layout=GridLayout { viewport_height: 720.0, row_height: 60.0, header_height: 64.0, overscan: 8 }
                            header_class="thead"
                            row_class=stripe
                        />
                    }
                    .to_html(),
                }
            };
            assert_eq!(render(Some(range)), render(None));
            // Untouched on the server: the scroller's writer is an Effect.
            assert_eq!(range.get_untracked(), (0, 0));
        });
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::grid`
Expected: compile errors — `HeaderLine2` has no field `header_class` / expected `HeaderPill`, found `Option<…>`; `AnalyzerGrid` has no prop `visible_range`.

- [ ] **Step 3: The two struct changes**

```rust
/// Line 2 of a header: `‹short signal› · ‹place›`, `"(= Cost / unit)"`, or
/// the window and source of a market column (`"7d · Gilgamesh"`). The pill
/// is the alternative-signal columns' "use" button; a column that has no
/// formula input to write leaves it `None` and renders text only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderLine2 {
    pub sub_label: String,
    pub pill: Option<HeaderPill>,
}

/// What a page hangs off a header: a hover title, optionally line 2, and
/// optionally the classes to use *while this extra is in effect* — a column
/// that becomes two-line only under a lab cannot carry the two-line width
/// in its static `header_class` without moving the flag-off DOM. Columns
/// with no entry render exactly as they did before this existed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderExtra {
    pub title: String,
    pub line2: Option<HeaderLine2>,
    pub header_class: Option<&'static str>,
}

/// Line 2's own classes on a header the grid draws itself (an unsortable
/// one). Identical to what `SortableHeaderCell` puts on its sub-label, so
/// the two kinds of header line up.
const HEADER_SUB_LINE: &str = "text-[10px] leading-3 font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full";
```

- [ ] **Step 4: `header_cell` reads the extra once and gains three arms**

```rust
fn header_cell<T: 'static, M: SortColumn>(
    col: &'static ToolColumnMeta<T, M>,
    sort_mode: Signal<Option<M>>,
    sort_dir: Signal<Option<SortDir>>,
    i18n: I18nContext<Locale, I18nKeys>,
    marks: Option<Signal<Option<MarkLabels>>>,
    extras: Option<Signal<HeaderExtras>>,
    on_pill: Option<Callback<ColumnKind>>,
) -> AnyView {
    let label_fn = col.spec.label;
    let kind = col.spec.kind;
    let role = marked_role(col, marks);
    let class = marked_class(role.is_some(), col.formula_header_class, col.header_class);
    // One lookup for every path. The marked arm ignores it, so those
    // columns and the always-on unsortable ones gain a subscription on
    // `header_extras` they did not have: free while the toggle is off (the
    // memo is a constant empty map and `Memo` suppresses equal values) and
    // one re-render per sell-world change with it on. The map is keyed by
    // kind, so no iteration order can reach the DOM.
    let extra = extras.and_then(|e| e.with(|e| e.by_kind.get(&kind).cloned()));
    match (col.sort, role) {
        // Marked: the badge names the operator, the sub-label says which
        // price this is, and the tint plus hairline tie it to the strip.
        // KEEP `grid.rs:169-189` CHARACTER FOR CHARACTER — the whole
        // `SortableHeaderCell` with `badge=role`, the `format!("{class}
        // truncate")` class, the marks-driven `sub_label` and
        // `emphasized`. It is elided here only so this block stays
        // readable; pasting an empty `view!` would delete the marked
        // header and leave `mode` and `role` unused (`-D warnings`).
        (Sortability::By(mode), Some(role)) => { /* grid.rs:169-189, verbatim */ }
        (Sortability::By(mode), None) => match extra {
            None => view! {
                <SortableHeaderCell mode=mode label=label_fn(i18n) class=col.header_class sort_mode sort_dir />
            }
            .into_any(),
            Some(HeaderExtra { title, line2: None, header_class }) => view! {
                <SortableHeaderCell mode=mode label=label_fn(i18n) title=title class=header_class.unwrap_or(col.header_class) sort_mode sort_dir />
            }
            .into_any(),
            Some(HeaderExtra { title, line2: Some(HeaderLine2 { sub_label, pill: None }), header_class }) => view! {
                <SortableHeaderCell
                    mode=mode
                    label=label_fn(i18n)
                    title=title
                    class=header_class.unwrap_or(col.header_class)
                    sort_mode
                    sort_dir
                    sub_label=Signal::derive(move || sub_label.clone())
                />
            }
            .into_any(),
            Some(HeaderExtra { title, line2: Some(HeaderLine2 { sub_label, pill: Some(pill) }), header_class }) => view! {
                <SortableHeaderCell
                    mode=mode
                    label=label_fn(i18n)
                    title=title
                    class=format!("{} truncate", header_class.unwrap_or(col.header_class))
                    sort_mode
                    sort_dir
                    sub_label=Signal::derive(move || sub_label.clone())
                    trailing=ViewFn::from(move || pill_view(kind, pill.clone(), on_pill, i18n))
                />
            }
            .into_any(),
        },
        // Unsortable headers were `t!(..)` on the page (locale-reactive);
        // keep that by resolving the label inside a closure. A lazy column
        // is unsortable for a different reason and renders the same way. A
        // pill on one of these would have no formula input to write, so
        // line 2 renders its text only.
        (Sortability::No | Sortability::LazyNever, _) => match extra {
            None => view! {
                <div role="columnheader" class=class>{move || label_fn(i18n)}</div>
            }
            .into_any(),
            Some(HeaderExtra { title, line2: None, header_class }) => view! {
                <div role="columnheader" class=header_class.unwrap_or(class) title=title>
                    {move || label_fn(i18n)}
                </div>
            }
            .into_any(),
            Some(HeaderExtra { title, line2: Some(HeaderLine2 { sub_label, .. }), header_class }) => view! {
                <div role="columnheader" class=header_class.unwrap_or(class) title=title>
                    <span>{move || label_fn(i18n)}</span>
                    <span class=HEADER_SUB_LINE>{sub_label}</span>
                </div>
            }
            .into_any(),
        },
    }
}
```

The `None` arms are the pre-existing markup character for character — that is what keeps the toggle-off header identical, and `unsortable_headers_take_a_title_and_a_second_line` plus the pre-existing `header_extras_render_title_sub_label_and_pill` assert it from both sides.

- [ ] **Step 5: The `visible_range` prop**

After `lab_columns`:

```rust
    /// Writeback of the rendered row range `(start, end)`, forwarded to the
    /// scroller so the page can fetch data only for rows in view. Omitted,
    /// the grid keeps a range signal of its own that nothing reads: the
    /// scroller's prop is `#[prop(optional, into)]` on an `Option`, which
    /// strips the `Option`, so there is no `None` to forward.
    #[prop(optional)]
    visible_range: Option<RwSignal<(usize, usize)>>,
```

and in the body, above the `view!`:

```rust
    let range = visible_range.unwrap_or_else(|| RwSignal::new((0, 0)));
```

with `visible_range=range` added to the `<VirtualScroller …>` call. The scroller's writer is a client `Effect` that only sets a signal, so a grid that ignores the range renders exactly what it rendered before.

- [ ] **Step 6: Keep the recipe page's four extras compiling**

In `recipe_analyzer.rs`'s `header_extras` memo, the `RevSignal` and `CostSignal` literals wrap their pill in `Some(..)` and every one of the four gains `header_class: None`:

```rust
                        pill: Some(HeaderPill {
                            aria: t_string!(
                                i18n,
                                analyzer_use_as_revenue_aria,
                                signal = signal_label(i18n, s)
                            )
                            .to_string(),
                            pressed: s == selected_revenue,
                        }),
                    }),
                    header_class: None,
                },
```

- [ ] **Step 7: Run the tests**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::grid`
Expected: PASS, 9 tests (6 plus the three added here: `a_second_line_without_a_pill_renders_no_button`, `unsortable_headers_take_a_title_and_a_second_line`, `visible_range_is_optional_and_changes_no_markup`). `lab_columns_are_absent_from_the_header_unless_enabled`, `header_extras_render_title_sub_label_and_pill` and the row-clip fix's `row_min_width_reaches_the_scrollers_spacer` all still pass: one flag, one bool, same filter, and the new prop is additive.

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer`
Expected: PASS, 44 tests.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/grid.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(analyzer-kit): grid visible_range prop, pill-less header line 2, titles on unsortable headers"
```

---
### Task 6: `needed.rs` — the 30-day sell-world body joins the gate

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/needed.rs:9-12` (`STATS_30_WINDOW_DAYS`), `:26-37` (`RecipeNeeds.stats_30`), `:41-59` (`needed_bodies`), `:145-151` (the test helper), and one new test
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:3369-3376` (the buy-stats memo's `RecipeNeeds` literal gains the field)

**Interfaces:**
- Consumes: `BodyRole::SellWorldStats(u16)` (`needed.rs:19`), already window-parameterised.
- Produces:
  - `pub const STATS_30_WINDOW_DAYS: u16 = 30;`
  - `RecipeNeeds.stats_30: bool`.
  - `needed_bodies` inserting `BodyRole::SellWorldStats(STATS_30_WINDOW_DAYS)` for it. Read by the page's `stats_30_key` in Task 8.

- [ ] **Step 1: Write the failing test**

In `needed.rs`'s `mod tests`, after `outlier_filter_needs_recent_sales`:

```rust
    #[test]
    fn thirty_day_columns_need_a_second_sell_world_body() {
        let f = ProfitFormula::recipe_from_query(None, None, None);
        let base = needed_bodies(&f, &needs(false, false));
        assert!(!base.contains(&BodyRole::SellWorldStats(STATS_30_WINDOW_DAYS)));
        let wants = RecipeNeeds {
            stats_30: true,
            ..needs(false, false)
        };
        let got = needed_bodies(&f, &wants);
        assert!(got.contains(&BodyRole::SellWorldStats(STATS_30_WINDOW_DAYS)));
        // Two windows are two bodies: the 7-day one is still needed.
        assert!(got.contains(&BodyRole::SellWorldStats(SALE_STATS_WINDOW_DAYS)));
        assert_eq!(got.len(), base.len() + 1);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::needed`
Expected: compile error — `cannot find value STATS_30_WINDOW_DAYS`, `RecipeNeeds` has no field `stats_30`.

- [ ] **Step 3: Add the window, the flag and the rule**

Beside `SALE_STATS_WINDOW_DAYS`:

```rust
/// The second window, read only by the opt-in 30-day columns. Its body is
/// client-only: it never joins the Suspense gate, so a page that wants it
/// still renders its table first and fills those two cells in after
/// (`LateStats`).
pub const STATS_30_WINDOW_DAYS: u16 = 30;
```

`RecipeNeeds` gains, after `cost_signals`:

```rust
    /// A 30-day column (Volume 30d, VWAP 30d) is visible or the sort
    /// target. Not "the lab is on": the body costs 438 KB on the wire, so
    /// only actually asking for one of those columns fetches it.
    pub stats_30: bool,
```

and `needed_bodies` gains, after the `outliers` rule:

```rust
    if needs.stats_30 {
        set.insert(BodyRole::SellWorldStats(STATS_30_WINDOW_DAYS));
    }
```

- [ ] **Step 4: Keep the two exhaustive literals compiling**

`needed.rs`'s test helper (`:145-151`) gains `stats_30: false`, and `recipe_analyzer.rs`'s buy-stats memo (`:3371-3375`) becomes:

```rust
        let needs = RecipeNeeds {
            outliers: false,
            buy_scope_is_sell_world: buy_scope_is_sell_world.get(),
            cost_signals: needs_page.get().cost,
            stats_30: false,
        };
```

with the comment above it noting that this memo answers the *buy-scope* body only — the sell-world 30-day body has its own key in Task 8.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::needed`
Expected: PASS, 10 tests (9 + `thirty_day_columns_need_a_second_sell_world_body`). `needed_bodies_default_is_todays_three_bodies` still passes: the default `RecipeNeeds` asks for no 30-day body.

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer`
Expected: PASS, 44 tests.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/needed.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(analyzer-kit): needed_bodies gates the client-only 30-day sell-world body"
```

---
### Task 7: Five columns on the recipe table — `stat_hq`, three sort modes, the 22/24 URL contract

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs`: imports (`:1-60`), `RecipeProfitData.stat_hq` (`:88-130`), `price_rows`' stat lookup (`:1790-1822`), the token consts (`:525-545`), five label fns (`:592-654`), five `ColumnSpec` statics (`:656-781`), the cell extractors (`:785-885`), the class consts (`:887-905`), `RECIPE_COLUMNS` 25 → 30 (`:927-1215`), `SortMode` + `lab_only` (`:1404-1435`), `compare_recipes` + `effective_sort_mode` (`:1479-1525`), `filter_and_sort` (`:1841-1880`), the `computed_data` memo's call (`:2268-2279`), and the tests (`:3814-4972`)

**Interfaces:**
- Consumes: `Layer::Lazy`, `LazyFeed`, `Sortability::LazyNever`, `ColumnKind::{ProfitPerDay, Trend, DriftSpark, VolumeUnits30, Vwap30}`, `PickerGroup::Market`, `CellCtx.{sparklines, stats_30}` (Task 3); `CellValue::{Sparkline, LazyPct, LateCount, LateGilWithPct}` (Task 4); `analysis::profit_per_day_from_rate` (Task 2); `signals::{StatsIndex, stat_row_either}` (Task 3); `LAB_ANALYZER_RECIPE` (Task 1).
- Produces:
  - `RecipeProfitData.stat_hq: bool` — the quality the row's 7-day statistics resolved to, and therefore the key both the sparkline feed and the 30-day lookups use.
  - `const RECIPE_TREND_FEED: LazyFeed = LazyFeed::Sparklines { hours: 168 };` — the Trend and Drift rows' layer and (Task 8) the fetch's `hours`.
  - Five `?cols=` tokens, five specs, five rows, five extractors, `SortMode::{ProfitPerDay, Volume30, Vwap30}`.
  - `fn effective_sort_mode(mode: SortMode, stats_30_loaded: bool) -> SortMode` and `filter_and_sort(.., stats_30: Option<&StatsIndex>)`, read by the table's memo (Task 8).

- [ ] **Step 1: Write the failing tests**

Rewrite the four contract tests and add four. First, the contracts:

```rust
    const ALL_SORT_MODES: [SortMode; 24] = [
        // … the 21 at HEAD, unchanged, then:
        SortMode::ProfitPerDay,
        SortMode::Volume30,
        SortMode::Vwap30,
    ];
```

```rust
    #[test]
    fn recipe_optional_column_order_is_a_stable_url_contract() {
        assert_eq!(
            OPTIONAL_COLUMN_ORDER.as_slice(),
            &[
                "confidence",
                "last-sold",
                "volume",
                "vwap",
                "tax",
                "listing-world",
                "listing-dc",
                "rev-listing-min",
                "rev-sale-min",
                "rev-sale-median",
                "rev-sale-avg",
                "cost-listing-min",
                "cost-sale-min",
                "cost-sale-median",
                "cost-sale-avg",
                "hop-gain",
                "hop-worlds",
                // Phase E2, appended so every serialized old URL stays
                // byte-identical.
                "profit-per-day",
                "trend",
                "drift",
                "volume-30d",
                "vwap-30d",
            ]
        );
        // The contract the page uses while the toggle is off: the seven of Phase B.
        assert_eq!(
            BASE_COLUMN_ORDER.as_slice(),
            &[
                "confidence",
                "last-sold",
                "volume",
                "vwap",
                "tax",
                "listing-world",
                "listing-dc"
            ]
        );
        assert_eq!(DEFAULT_COLS.as_slice(), &["confidence"]);
    }
```

`sort_mode_round_trips_through_the_url` keeps its body and gains three assertions:

```rust
        assert_eq!(SortMode::ProfitPerDay.to_string(), "profit-per-day");
        assert_eq!(SortMode::Volume30.to_string(), "volume-30d");
        assert_eq!("vwap-30d".parse::<SortMode>(), Ok(SortMode::Vwap30));
```

`signal_columns_have_unique_ids_and_sort_tokens` moves to the new counts:

```rust
        assert_eq!(n_ids, 22);
        assert_eq!(
            n_sorts, 24,
            "the eleven sorts at HEAD, the ten signal and hop columns, and E2's three; \
             listing world/dc, trend and drift do not sort"
        );
        for c in RECIPE_COLUMNS.iter().filter(|c| c.lab.is_some()) {
            assert!(!c.default_on, "{} must start hidden", c.id);
            assert_eq!(c.lab, Some(LAB_ANALYZER_RECIPE));
            assert!(
                c.header_class.contains("hidden md:"),
                "{}: desktop-only (kit decision 7)",
                c.id
            );
        }
        assert_eq!(
            RECIPE_COLUMNS.iter().filter(|c| c.lab.is_some()).count(),
            15
        );
```

`lab_only_sort_modes_are_exactly_the_ten` becomes:

```rust
    #[test]
    fn lab_only_sort_modes_are_exactly_the_thirteen() {
        assert_eq!(ALL_SORT_MODES.iter().filter(|m| m.lab_only()).count(), 13);
        assert!(!SortMode::CostPerUnit.lab_only() && !SortMode::Price.lab_only());
        assert!(SortMode::ProfitPerDay.lab_only() && SortMode::Vwap30.lab_only());
    }
```

and `picker_columns_are_a_subset_of_optional_column_order`'s `assert_eq!(ids.len(), 17)` becomes `22`.

Then the four new tests:

```rust
    fn test_ctx() -> CellCtx {
        CellCtx {
            now_unix: 1_700_000_000,
            preview: true,
            capped_cost: [false; 4],
            sparklines: None,
            stats_30: None,
        }
    }

    fn stats_row(item_id: i32, hq: bool, units_sold: u64, vwap: i32) -> ItemSaleStats {
        ItemSaleStats {
            item_id,
            hq,
            units_sold,
            vwap,
            ..Default::default()
        }
    }

    /// Profit/day is the row's profit times the 7-day rollup rate, computed
    /// in the cell and in the comparator from the same helper — no field,
    /// no fetch.
    #[test]
    fn profit_per_day_is_profit_times_the_rollup_rate() {
        let keys: Vec<i32> = fixture_recipes()
            .iter()
            .take(2)
            .map(|r| r.key_id.0)
            .collect();
        let fast = row(keys[0], 1_000, 0, 3.0, 1);
        let slow = row(keys[1], 1_000, 0, 0.25, 1);
        assert_eq!(cell_profit_per_day(&fast, &test_ctx()), CellValue::Gil(3_000));
        assert_eq!(cell_profit_per_day(&slow, &test_ctx()), CellValue::Gil(250));
        let out = filter_and_sort(
            &[slow, fast],
            &Thresholds::default(),
            &HashMap::new(),
            SortMode::ProfitPerDay,
            SortDir::Desc,
            None,
        );
        assert_eq!(
            out.iter().map(|(_, r)| r.recipe.key_id.0).collect::<Vec<_>>(),
            vec![keys[0], keys[1]],
            "the faster seller ranks first even at equal profit"
        );
    }

    /// A 30-day sort reads as Profit until the client-only body lands, then
    /// orders by the 30-day figure with the rows the body knows nothing
    /// about last in both directions.
    #[test]
    fn thirty_day_sorts_fall_back_to_profit_until_the_body_lands() {
        assert_eq!(effective_sort_mode(SortMode::Volume30, false), SortMode::Profit);
        assert_eq!(effective_sort_mode(SortMode::Vwap30, false), SortMode::Profit);
        assert_eq!(effective_sort_mode(SortMode::Volume30, true), SortMode::Volume30);
        assert_eq!(effective_sort_mode(SortMode::Profit, false), SortMode::Profit);

        let keys: Vec<i32> = fixture_recipes()
            .iter()
            .take(3)
            .map(|r| r.key_id.0)
            .collect();
        let rows = vec![
            row(keys[0], 10, 0, 1.0, 1),
            row(keys[1], 20, 0, 1.0, 1),
            row(keys[2], 30, 0, 1.0, 1),
        ];
        let mut index: StatsIndex = StatsIndex::new();
        index.insert(
            (rows[0].recipe.item_result, false),
            stats_row(rows[0].recipe.item_result, false, 500, 100),
        );
        index.insert(
            (rows[1].recipe.item_result, false),
            stats_row(rows[1].recipe.item_result, false, 900, 200),
        );
        // rows[2] is not in the 30-day body at all.
        let order = |dir, index: Option<&StatsIndex>| {
            filter_and_sort(
                &rows,
                &Thresholds::default(),
                &HashMap::new(),
                SortMode::Volume30,
                dir,
                index,
            )
            .into_iter()
            .map(|(_, r)| r.recipe.key_id.0)
            .collect::<Vec<_>>()
        };
        assert_eq!(order(SortDir::Desc, Some(&index)), vec![keys[1], keys[0], keys[2]]);
        assert_eq!(order(SortDir::Asc, Some(&index)), vec![keys[0], keys[1], keys[2]]);
        // No body yet: profit order, not "every row equal".
        assert_eq!(order(SortDir::Desc, None), vec![keys[2], keys[1], keys[0]]);
        // A failed fetch stores an empty index: still profit order, never
        // the recipe-id order an all-`None` comparison would leave behind.
        assert_eq!(
            order(SortDir::Desc, Some(&StatsIndex::new())),
            vec![keys[2], keys[1], keys[0]]
        );
    }

    /// The lazy pair is unreachable from a URL and unreachable from a
    /// header click: no `?sort=` token, `Sortability::LazyNever`.
    #[test]
    fn the_lazy_columns_never_sort() {
        for id in [COL_TREND, COL_DRIFT] {
            let col = RECIPE_COLUMNS
                .iter()
                .find(|c| c.id == id)
                .expect("column in the table");
            assert_eq!(col.sort, Sortability::LazyNever, "{id}");
            assert!(col.sort_id.is_empty(), "{id}");
        }
        assert!("trend".parse::<SortMode>().is_err());
        assert!("drift".parse::<SortMode>().is_err());
    }

    /// The row records which quality its 7-day statistics came from, so the
    /// sparkline key and the 30-day lookups read the same quality the
    /// visible 7-day numbers did.
    #[test]
    fn stat_hq_records_the_quality_the_row_priced_from() {
        let nq = run(PriceSignal::ListingMin, PriceSignal::ListingMin, false);
        assert!(nq.iter().all(|r| !r.stat_hq), "the fixture trades NQ");
        let f = ProfitFormula::recipe_from_query(Some(PriceSignal::ListingMin), None, None);
        let hq = run_with(
            PriceSignal::ListingMin,
            PriceSignal::ListingMin,
            &RunOpts {
                stats_hq: true,
                needs: needed_signals(&f, &SignalWants::default(), false),
                ..RunOpts::default()
            },
        );
        // `require_hq` is false, so the pass falls back to the HQ row — and
        // says so on the row.
        assert!(hq.iter().any(|r| r.stat_hq), "some rows have only an HQ row");
        // And the row's figures came from that same lookup: the remapped
        // fixture rows are the only ones carrying a vwap or a unit count.
        assert!(
            hq.iter()
                .filter(|r| r.stat_hq)
                .all(|r| r.vwap > 0 && r.units_sold == 3),
            "the HQ row's figures are what the row carries"
        );
        assert!(hq.iter().filter(|r| !r.stat_hq).all(|r| r.vwap == 0));
    }
```

with `RunOpts` gaining `stats_hq: bool` — it has a **hand-written** `impl Default` (`recipe_analyzer.rs:4278-4288`), so add `stats_hq: false` there too, or `..RunOpts::default()` in the new test will not compile — and `run_with` building a second index for the sell side only (`buy_stats` keeps the plain one; the test prices ListingMin/ListingMin, but a cost-side swap would be an unrelated variable):

```rust
        let index = stats_index(&stats);
        let sell_index = if o.stats_hq {
            // The same rows, only HQ, and carrying the two figures the row
            // copies off its stat row: exercises the pass's fallback to the
            // other quality when the required one never traded, and lets a
            // test tell which row the numbers came from.
            stats
                .stats
                .iter()
                .map(|s| {
                    (
                        (s.item_id, true),
                        ItemSaleStats {
                            hq: true,
                            vwap: s.avg_price,
                            units_sold: 3,
                            ..*s
                        },
                    )
                })
                .collect()
        } else {
            index.clone()
        };
```

and its `PriceInputs` reading `sell_stats: if o.sell_stats { &sell_index } else { &empty_index }` while `buy_stats: Some(&index)` stays as it is. (`ItemSaleStats` is `Copy`, so `..*s` is valid.)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer`
Expected: compile errors — `no variant ProfitPerDay on SortMode`, `cannot find function cell_profit_per_day` / `effective_sort_mode`, `filter_and_sort` takes 5 arguments, `RecipeProfitData` has no field `stat_hq`, `RunOpts` has no field `stats_hq`.

- [ ] **Step 3: The row records its stat quality**

`RecipeProfitData` gains, after `confidence`:

```rust
    /// Which quality the sell-world statistics above came from: the
    /// required one, or the other when only that one traded. The lazy
    /// sparkline feed and the 30-day columns key on it, so every figure in
    /// a row describes the same quality.
    stat_hq: bool,
```

and `price_rows` (`:1790-1796`) uses the kit's rule and records the answer:

```rust
        // Sell-world stats row matching how revenue resolves: prefer
        // the HQ row when the analyzer requires HQ, otherwise NQ, and
        // fall back to whichever quality actually traded.
        let sell_stat = stat_row_either(inp.sell_stats, recipe.item_result, inp.require_hq);
        let stat_hq = sell_stat.map(|s| s.hq).unwrap_or(inp.require_hq);
        let vwap = sell_stat.map(|s| s.vwap).unwrap_or(0);
```

with `stat_hq,` added to the `RecipeProfitData` literal (`:1798-1822`) and `stat_hq: false` to the `row(..)` test fixture (`:4556-4586`). `price_rows_matches_recorded_oracle_on_fixture` projects each row to `(key_id, profit, roi, cost, market_price, tax)` and cannot see the new field; `stat_row_either` is the same two-step lookup written once, so no number moves.

- [ ] **Step 4: Tokens, specs, extractors, classes**

Token consts, after `COL_HOP_WORLDS`:

```rust
// Phase E2's market columns, appended after the ten above for the same
// reason: an old serialized `?cols=` must round-trip byte-identically.
const COL_PROFIT_PER_DAY: &str = "profit-per-day";
const COL_TREND: &str = "trend";
const COL_DRIFT: &str = "drift";
const COL_VOLUME_30D: &str = "volume-30d";
const COL_VWAP_30D: &str = "vwap-30d";

/// The lazy feed the Trend and Drift columns share: 168 hourly points, one
/// request per visible window. `RECIPE_TREND_FEED.hours()` is what the
/// fetch sends, so the column table and the request can never disagree.
const RECIPE_TREND_FEED: LazyFeed = LazyFeed::Sparklines { hours: 168 };
```

Labels (reusing the flip finder's three column names verbatim — same definitions, same words):

```rust
fn label_profit_per_day(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_profit_per_day).to_string()
}
fn label_trend(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_spark).to_string()
}
fn label_drift(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, analyzer_col_drift).to_string()
}
fn label_volume_30d(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, recipe_analyzer_col_volume_30d).to_string()
}
fn label_vwap_30d(i18n: I18nContext<Locale, I18nKeys>) -> String {
    t_string!(i18n, recipe_analyzer_col_vwap_30d).to_string()
}
```

Specs, all in the new Market group:

```rust
static SPEC_PROFIT_PER_DAY: ColumnSpec = ColumnSpec {
    kind: ColumnKind::ProfitPerDay,
    label: label_profit_per_day,
    group: PickerGroup::Market,
};
static SPEC_TREND: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Trend,
    label: label_trend,
    group: PickerGroup::Market,
};
static SPEC_DRIFT: ColumnSpec = ColumnSpec {
    kind: ColumnKind::DriftSpark,
    label: label_drift,
    group: PickerGroup::Market,
};
static SPEC_VOLUME_30D: ColumnSpec = ColumnSpec {
    kind: ColumnKind::VolumeUnits30,
    label: label_volume_30d,
    group: PickerGroup::Market,
};
static SPEC_VWAP_30D: ColumnSpec = ColumnSpec {
    kind: ColumnKind::Vwap30,
    label: label_vwap_30d,
    group: PickerGroup::Market,
};
```

Extractors, after `cell_hop_gain`:

```rust
fn cell_profit_per_day(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::Gil(profit_per_day_from_rate(r.profit, r.daily_sales))
}

/// One read of the page's sparkline store, projected. The read happens
/// inside the row's reactive closure, which is what makes the cell re-render
/// when a batch merges; with no store (every other page, and every test)
/// the cell stays on its loading shape, which is also the server's.
fn spark_with<V>(r: &RecipeRow, ctx: &CellCtx, f: impl Fn(&SparkValue) -> V) -> Enrich<V> {
    let key = (r.recipe.item_result, r.stat_hq);
    match ctx.sparklines {
        Some(store) => store.with(|s| s.state(&key).map(f)),
        None => Enrich::Loading,
    }
}

fn cell_trend(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    CellValue::Sparkline(spark_with(r, ctx, SparkValue::clone))
}

fn cell_drift(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    CellValue::LazyPct(spark_with(r, ctx, |v| v.delta_pct))
}

/// The same, for the client-only 30-day body: `Loading` while it is in
/// flight, `Missing` once it has landed with no row for this item (and on a
/// page that has no such body).
fn late_30<V>(r: &RecipeRow, ctx: &CellCtx, f: impl Fn(&ItemSaleStats) -> V) -> Enrich<V> {
    let Some(stats) = ctx.stats_30 else {
        return Enrich::Missing;
    };
    stats.with(|index| match index {
        None => Enrich::Loading,
        Some(index) => match stat_row_either(index, r.recipe.item_result, r.stat_hq) {
            Some(row) => Enrich::Ready(f(row)),
            None => Enrich::Missing,
        },
    })
}

fn cell_volume_30(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    CellValue::LateCount(late_30(r, ctx, |s| s.units_sold))
}

fn cell_vwap_30(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    let price = r.market_price;
    CellValue::LateGilWithPct(late_30(r, ctx, move |s| (s.vwap, vwap_pct(price, s.vwap))))
}
```

Classes, beside the existing ones:

```rust
/// The two lazy columns' headers: the grid draws these itself (they are
/// unsortable), so the class carries `flex flex-col` for the label and its
/// "7d · ‹sell world›" line. `md:flex`, never `md:block`, for the same
/// reason `HEAD_40_MD` is.
const HEAD_LAZY_MD: &str = "w-28 shrink-0 px-4 py-2 leading-tight hidden md:flex flex-col";
const HEAD_LAZY_MD_END: &str =
    "w-28 shrink-0 px-4 py-2 leading-tight hidden md:flex flex-col items-end";
/// A cell that centres a fixed-width graphic (the 80 px sparkline in a
/// `w-28` column, the same 16 px of padding either side as the numbers).
const CELL_28_MID_MD: &str = "px-4 py-2 w-28 shrink-0 hidden md:flex items-center justify-center";
/// A right-aligned numeric cell, as the 7-day Volume column already spells
/// it inline.
const CELL_28_NUM_MD: &str =
    "px-4 py-2 w-28 shrink-0 text-right hidden md:block font-mono tabular-nums";
```

- [ ] **Step 5: Five table rows**

`static RECIPE_COLUMNS: [ToolColumnMeta<RecipeRow, SortMode>; 30]`, the five appended after `SPEC_HOP_WORLDS` and before `SPEC_ACTIONS`:

```rust
    ToolColumnMeta {
        spec: &SPEC_PROFIT_PER_DAY,
        id: COL_PROFIT_PER_DAY,
        sort_id: COL_PROFIT_PER_DAY,
        sort: sortability_for(Layer::Computed, Some(SortMode::ProfitPerDay)),
        header_class: HEAD_MD,
        cell_class: CELL_R_MD,
        default_on: false,
        cell: cell_profit_per_day,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_TREND,
        id: COL_TREND,
        // Lazy: fetched per visible window, so it never sorts and carries
        // no `?sort=` token.
        sort: sortability_for(Layer::Lazy(RECIPE_TREND_FEED), None),
        header_class: HEAD_LAZY_MD,
        cell_class: CELL_28_MID_MD,
        default_on: false,
        cell: cell_trend,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_DRIFT,
        id: COL_DRIFT,
        // The same feed, read as a first-to-last percent: one request
        // serves both columns.
        sort: sortability_for(Layer::Lazy(RECIPE_TREND_FEED), None),
        header_class: HEAD_LAZY_MD_END,
        cell_class: CELL_28_NUM_MD,
        default_on: false,
        cell: cell_drift,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_VOLUME_30D,
        id: COL_VOLUME_30D,
        sort_id: COL_VOLUME_30D,
        // Bulk: a whole-scope body, even though this one is fetched
        // client-side after the table (`needed.rs`'s SellWorldStats(30)).
        sort: sortability_for(Layer::Bulk, Some(SortMode::Volume30)),
        header_class: HEAD_28_MD,
        cell_class: CELL_28_NUM_MD,
        default_on: false,
        cell: cell_volume_30,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
    ToolColumnMeta {
        spec: &SPEC_VWAP_30D,
        id: COL_VWAP_30D,
        sort_id: COL_VWAP_30D,
        sort: sortability_for(Layer::Bulk, Some(SortMode::Vwap30)),
        header_class: HEAD_MD,
        cell_class: CELL_R_MD,
        default_on: false,
        cell: cell_vwap_30,
        lab: Some(LAB_ANALYZER_RECIPE),
        ..RECIPE_BASE
    },
```

- [ ] **Step 6: Three sort modes, the comparator and the fallback**

`SortMode` gains three variants after `HopWorlds`:

```rust
    /// Profit times the 7-day rollup rate. Computed per comparison; there
    /// is no row field to keep in sync.
    ProfitPerDay,
    /// Units sold over 30 days, from the client-only body.
    Volume30,
    /// The 30-day volume-weighted average price, from the same body.
    Vwap30,
```

`lab_only` gains them:

```rust
    fn lab_only(self) -> bool {
        matches!(
            self,
            SortMode::RevSignal(_)
                | SortMode::CostSignal(_)
                | SortMode::HopGain
                | SortMode::HopWorlds
                | SortMode::ProfitPerDay
                | SortMode::Volume30
                | SortMode::Vwap30
        )
    }
```

`compare_recipes` takes the 30-day index and gains three arms:

```rust
/// A row's 30-day statistics, when that body has landed. Keyed on the same
/// quality the row's 7-day figures came from.
fn stat_30<'a>(index: Option<&'a StatsIndex>, r: &RecipeProfitData) -> Option<&'a ItemSaleStats> {
    stat_row_either(index?, r.recipe.item_result, r.stat_hq)
}

fn compare_recipes(
    mode: SortMode,
    dir: SortDir,
    a: &RecipeProfitData,
    b: &RecipeProfitData,
    stats_30: Option<&StatsIndex>,
) -> Ordering {
    // … the existing arms …
        SortMode::ProfitPerDay => oriented(
            profit_per_day_from_rate(a.profit, a.daily_sales)
                .cmp(&profit_per_day_from_rate(b.profit, b.daily_sales)),
        ),
        SortMode::Volume30 => cmp_none_last(
            stat_30(stats_30, a).map(|s| s.units_sold),
            stat_30(stats_30, b).map(|s| s.units_sold),
            dir,
            u64::cmp,
        ),
        SortMode::Vwap30 => cmp_none_last(
            stat_30(stats_30, a).map(|s| s.vwap).filter(|v| *v > 0),
            stat_30(stats_30, b).map(|s| s.vwap).filter(|v| *v > 0),
            dir,
            i32::cmp,
        ),
}
```

and the fallback, beside it:

```rust
/// The mode the rows are actually sorted by. The 30-day body is client-only
/// and lands after the first paint; sorting the whole table by "nothing
/// yet" would leave it in key order and then shuffle it, so until the body
/// arrives *with rows* a 30-day sort reads as Profit. "With rows", not
/// merely "present": a failed fetch and a world with no 30-day history both
/// store an empty index, which sorts no better than nothing. The header
/// still shows what was asked for, and the table re-sorts itself the moment
/// real rows land.
fn effective_sort_mode(mode: SortMode, stats_30_loaded: bool) -> SortMode {
    match mode {
        SortMode::Volume30 | SortMode::Vwap30 if !stats_30_loaded => SortMode::Profit,
        other => other,
    }
}
```

`filter_and_sort` gains the parameter and applies both:

```rust
fn filter_and_sort(
    rows: &[Arc<RecipeProfitData>],
    t: &Thresholds,
    world_names: &HashMap<i32, (String, String)>,
    mode: SortMode,
    dir: SortDir,
    stats_30: Option<&StatsIndex>,
) -> Vec<(usize, Arc<RecipeProfitData>)> {
    // A failed fetch stores an *empty* index (Task 8) so the cells settle to
    // "—" rather than shimmering; for sorting that is still "not landed",
    // because every key would compare `None` against `None`, leave the
    // key-id tiebreak in charge, and put the table in recipe-id order.
    let stats_30 = stats_30.filter(|i| !i.is_empty());
    let mode = effective_sort_mode(mode, stats_30.is_some());
    // … the filters, unchanged …
    kept.sort_by(|a, b| {
        compare_recipes(mode, dir, a, b, stats_30)
            .then_with(|| a.recipe.key_id.0.cmp(&b.recipe.key_id.0))
    });
```

The `computed_data` memo passes `None` for now; Task 8 wires the real signal. Every `filter_and_sort` call in `mod test` gains a trailing `None` — four of them (`grep -n 'filter_and_sort('`): three in `filter_and_sort_is_pure_and_inclusive` and its listing-filter sibling, plus one inside `hop_needed_sorts_last_both_directions`' `order` closure.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer`
Expected: PASS, 48 tests (44 + `profit_per_day_is_profit_times_the_rollup_rate`, `thirty_day_sorts_fall_back_to_profit_until_the_body_lands`, `the_lazy_columns_never_sort`, `stat_hq_records_the_quality_the_row_priced_from`). `price_rows_matches_recorded_oracle_on_fixture` passes unchanged — the proof that the `stat_row_either` refactor moved no number.

Run: `cargo test -p ultros-app --lib -- analyzer_kit`
Expected: PASS, 66 tests (unchanged by this task: it only consumes the kit).

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(recipe-analyzer): Profit/day, Trend, Drift, Volume 30d and VWAP 30d columns behind the toggle"
```

---
### Task 8: The page's market handles — the hook at page level, the 30-day body, the rows mirror

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs`: imports, `MarketHandles` + the feed helpers beside `RecipeRow` (`:582-590`), `stats_30_wanted` / `stats_30_key` beside `buy_stats_scope_key` (`:1267-1278`), the table's new prop (`:1916-1976`), the rows-mirror `Effect` and the `computed_data` memo (`:2268-2280`), `cell_ctx` (`:2766-2772`), the grid call (`:3086-3106`), the page's handles / hook / 30-day `Effect` (after `sell_world_name`, `:3429-3470`), the `<RecipeAnalyzerTable …/>` call (`:3643-3666`), and four tests

**Interfaces:**
- Consumes: `use_visible_enrichment`, `EnrichmentConfig`, `PREFETCH_MARGIN`, `DEBOUNCE_MS`, `verdict`, `Verdict` (`analyzer_kit::enrichment`); `SparkKey`, `SparkValue`, `SparkStore` (Task 3); `LateStats`, `stats_index`, `StatsIndex` (kit `signals`); `STATS_30_WINDOW_DAYS`, `RecipeNeeds.stats_30`, `needed_bodies` (Task 6); `filter_and_sort(.., stats_30)` and `RECIPE_TREND_FEED` (Task 7); `post_sparklines`, `get_sale_stats` (`api.rs`); the grid's `visible_range` prop (Task 5).
- Produces:
  - `struct MarketHandles { sparklines, stats_30, visible_range, rows }` (`Copy`), the table's one new prop.
  - `fn recipe_spark_key(&(usize, RecipeRow)) -> SparkKey`, `fn spark_entry(SparklineSeries) -> (SparkKey, SparkValue)`, `async fn fetch_recipe_sparklines(Option<String>, Vec<SparkKey>) -> Vec<(SparkKey, SparkValue)>`.
  - `const RECIPE_ENRICHMENT: EnrichmentConfig`, `const RECIPE_GRID: GridLayout`.
  - `fn stats_30_wanted(&HashSet<&'static str>, Option<SortMode>) -> bool`, `fn stats_30_key(&ProfitFormula, &RecipeNeeds, Option<&str>) -> Option<String>`.

- [ ] **Step 1: Write the failing tests**

In `recipe_analyzer.rs`'s `mod tests`:

```rust
    #[test]
    fn recipe_spark_key_is_item_and_stat_quality() {
        let keys: Vec<i32> = fixture_recipes().iter().take(1).map(|r| r.key_id.0).collect();
        let mut r = Arc::try_unwrap(row(keys[0], 0, 0, 1.0, 1)).ok().unwrap();
        assert_eq!(
            recipe_spark_key(&(0, Arc::new(r.clone()))),
            (r.recipe.item_result, false)
        );
        r.stat_hq = true;
        assert_eq!(
            recipe_spark_key(&(3, Arc::new(r.clone()))),
            (r.recipe.item_result, true),
            "the key follows the quality the row's statistics came from"
        );
    }

    /// One series in, one keyed value out: the colour driver is computed
    /// here, so the cell never scans the points.
    #[test]
    fn a_series_becomes_a_keyed_spark_value() {
        let up = SparklineSeries {
            item_id: 42,
            hq: true,
            world_id: 1,
            points: vec![100, 0, 150],
            first_price: 100,
            last_price: 150,
        };
        let (key, value) = spark_entry(up);
        assert_eq!(key, (42, true));
        assert_eq!(value.points, vec![100, 0, 150]);
        assert_eq!(value.delta_pct, Some(50.0));
        // Nothing traded anywhere in the window (`first_price` is the first
        // non-zero point): no percentage, so the sparkline reads neutral and
        // Drift shows "—".
        let quiet = SparklineSeries {
            item_id: 7,
            hq: false,
            world_id: 1,
            points: vec![0, 0],
            first_price: 0,
            last_price: 0,
        };
        assert_eq!(spark_entry(quiet).1.delta_pct, None);
    }

    /// The visible window is one request, derived from the grid's own
    /// geometry rather than a literal, and under the endpoint's 200-key cap.
    ///
    /// `chunk_keys` and `rows_for_viewport` are used only here, so import
    /// them **inside** `mod tests` — `use crate::analyzer_kit::enrichment::
    /// chunk_keys;` and `use crate::components::virtual_scroller::
    /// rows_for_viewport;` — exactly as `routes/analyzer.rs` does for its
    /// own window test. At module level they are unused in the non-test lib
    /// build that `--all-targets` also compiles, which `-D warnings` turns
    /// into a Task 10 failure.
    #[test]
    fn the_recipe_window_is_one_request_per_scroll_settle() {
        let rendered = rows_for_viewport(
            RECIPE_GRID.viewport_height - RECIPE_GRID.header_height,
            RECIPE_GRID.row_height,
            RECIPE_GRID.overscan,
        ) as usize;
        assert_eq!(rendered, 19, "11 rows in 656 px plus 8 overscan");
        let keys: Vec<SparkKey> = (0..rendered + 2 * PREFETCH_MARGIN)
            .map(|i| (i as i32, false))
            .collect();
        assert_eq!(keys.len(), 79);
        assert_eq!(
            chunk_keys(&keys, RECIPE_ENRICHMENT.max_keys_per_request).len(),
            1
        );
        assert_eq!(RECIPE_TREND_FEED.hours(), 168);
    }

    /// The 30-day body is fetched only when a 30-day column asks for it,
    /// and cannot be asked for at all with the toggle off.
    #[test]
    fn the_thirty_day_body_is_only_requested_when_a_30d_column_is() {
        let f = ProfitFormula::recipe_from_query(None, None, None);
        let idle = RecipeNeeds::default();
        assert_eq!(stats_30_key(&f, &idle, Some("Gilgamesh")), None);
        let wants = RecipeNeeds {
            stats_30: true,
            ..RecipeNeeds::default()
        };
        assert_eq!(
            stats_30_key(&f, &wants, Some("Gilgamesh")),
            Some("Gilgamesh".into())
        );
        // No sell world resolved yet: nothing to fetch from.
        assert_eq!(stats_30_key(&f, &wants, None), None);

        let on = parse_visible_cols(Some("volume-30d"), &OPTIONAL_COLUMN_ORDER, &DEFAULT_COLS);
        assert!(stats_30_wanted(&on, None));
        assert!(stats_30_wanted(&HashSet::new(), Some(SortMode::Vwap30)));
        assert!(!stats_30_wanted(&HashSet::new(), Some(SortMode::Profit)));
        // Toggle off: the token is not in the contract, so it never
        // survives parsing and the body is unreachable.
        let off = parse_visible_cols(Some("volume-30d"), &BASE_COLUMN_ORDER, &DEFAULT_COLS);
        assert!(!stats_30_wanted(&off, None));
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer`
Expected: compile errors — `cannot find function recipe_spark_key` / `spark_entry` / `stats_30_key` / `stats_30_wanted`, `cannot find value RECIPE_GRID` / `RECIPE_ENRICHMENT`.

- [ ] **Step 3: The handles, the key, the fetch and the two consts**

Beside `RecipeRow`'s `AnalyzerRow` impl:

```rust
/// The page-level handles E2's market columns read and write. Page-level,
/// not table-level, because the table remounts whenever one of its
/// resources changes — a cost-basis switch does — and the store, the hook's
/// claim set and the 30-day body all have to survive that. Only a sell-world
/// change resets them, which is exactly the hook's own rule.
#[derive(Copy, Clone)]
struct MarketHandles {
    /// Filled by `use_visible_enrichment`, called at page level.
    sparklines: RwSignal<SparkStore>,
    /// Filled by the page's 30-day `Effect`; `None` until it lands.
    stats_30: LateStats,
    /// Written by the scroller through the grid's `visible_range` prop.
    visible_range: RwSignal<(usize, usize)>,
    /// The table's sorted rows, mirrored for the hook. Empty unless Trend
    /// or Drift is visible, so the toggle-off page never fetches.
    rows: RwSignal<Vec<(usize, RecipeRow)>>,
}

/// The enrichment key: the item and the quality its statistics came from,
/// so one request serves Trend and Drift and both agree with the 7-day
/// numbers beside them.
fn recipe_spark_key((_, row): &(usize, RecipeRow)) -> SparkKey {
    (row.recipe.item_result, row.stat_hq)
}

/// One wire series to one stored value. The colour driver is computed here
/// (both ends are on the wire), so no cell ever scans the points.
fn spark_entry(s: SparklineSeries) -> (SparkKey, SparkValue) {
    (
        (s.item_id, s.hq),
        SparkValue {
            // Before `points`: the key and this field must be read while
            // `s` is whole, and `points` moves it.
            delta_pct: first_to_last_pct(s.first_price, s.last_price),
            points: s.points,
        },
    )
}

/// The visible window's sparkline fetch. A world that has not resolved yet
/// and a failed request both yield nothing; the hook settles every
/// requested key either way, so a cell goes loading → "—" rather than
/// shimmering forever. Only ever called from the hook's effect (`post_api`
/// is `unreachable!` under SSR).
async fn fetch_recipe_sparklines(
    world: Option<String>,
    keys: Vec<SparkKey>,
) -> Vec<(SparkKey, SparkValue)> {
    let Some(world) = world else {
        return Vec::new();
    };
    match post_sparklines(
        &world,
        SparklinesRequest {
            items: keys,
            hours: Some(RECIPE_TREND_FEED.hours()),
        },
    )
    .await
    {
        Ok(res) => res.series.into_iter().map(spark_entry).collect(),
        Err(_) => Vec::new(),
    }
}

const RECIPE_ENRICHMENT: EnrichmentConfig = EnrichmentConfig {
    prefetch_margin: PREFETCH_MARGIN,
    debounce_ms: DEBOUNCE_MS,
    // The sparklines endpoint rejects a request above 200 keys.
    max_keys_per_request: 200,
};

/// The grid's geometry, hoisted out of the `view!` so the window test
/// derives the batch size from the same numbers the scroller uses.
const RECIPE_GRID: GridLayout = GridLayout {
    viewport_height: 720.0,
    row_height: 60.0,
    header_height: 64.0,
    overscan: 8,
};
```

and, beside `buy_stats_scope_key`:

```rust
/// A 30-day column is visible or the sort target — the only reason to fetch
/// that body. Not "the toggle is on": with it off neither token survives
/// `parse_visible_cols` (the contract is `BASE_COLUMN_ORDER`) and neither
/// mode survives `SortMode::lab_only`, so this is false by construction.
fn stats_30_wanted(visible: &HashSet<&'static str>, sort: Option<SortMode>) -> bool {
    visible.contains(COL_VOLUME_30D)
        || visible.contains(COL_VWAP_30D)
        || matches!(sort, Some(SortMode::Volume30 | SortMode::Vwap30))
}

/// The 30-day body's key: the sell world's name when that body is needed,
/// `None` (no fetch) otherwise. Goes through `needed_bodies` like every
/// other body, so the gate lives in one place.
fn stats_30_key(
    formula: &ProfitFormula,
    needs: &RecipeNeeds,
    world: Option<&str>,
) -> Option<String> {
    needed_bodies(formula, needs)
        .contains(&BodyRole::SellWorldStats(STATS_30_WINDOW_DAYS))
        .then(|| world.map(str::to_string))
        .flatten()
}
```

- [ ] **Step 4: The page owns the handles, the hook and the 30-day Effect**

After `sell_world_name` (`:3429`):

```rust
    // E2's market columns. One set of handles for the page, so a table
    // remount keeps every settled sparkline key and the 30-day body.
    let market = MarketHandles {
        sparklines: RwSignal::new(SparkStore::default()),
        stats_30: RwSignal::new(None),
        visible_range: RwSignal::new((0, 0)),
        rows: RwSignal::new(Vec::new()),
    };
    // Trend and Drift: the flip finder's visible-window fetch, scoped to the
    // sell world (the sparklines endpoint takes a world, never a datacenter).
    // The hook's own effect resets the store when that world changes.
    use_visible_enrichment(
        market.sparklines,
        market.rows.into(),
        market.visible_range.into(),
        sell_world_name.into(),
        recipe_spark_key,
        fetch_recipe_sparklines,
        RECIPE_ENRICHMENT,
    );

    // The 30-day statistics body: client-only, one per sell world, fetched
    // the first time a 30-day column is visible or the sort target and kept
    // across column toggles. Never a `Resource`: it must not join the
    // Suspense gate, or the whole table would wait 700 ms for a column two
    // players use.
    let stats_30_source = Memo::new(move |_| {
        stats_30_key(
            &formula_page.get(),
            &RecipeNeeds {
                stats_30: stats_30_wanted(&visible_cols.get(), sort_mode.get()),
                ..RecipeNeeds::default()
            },
            sell_world_name.get().as_deref(),
        )
    });
    let stats_30_fetching = StoredValue::new(false);
    let stats_30_world = StoredValue::new(None::<String>);
    Effect::new(move |_| {
        let world = sell_world_name.get();
        // A world change drops the stored body even when nothing wants one
        // right now: it describes the old world.
        if stats_30_world.get_value() != world {
            stats_30_world.set_value(world);
            market.stats_30.set(None);
            stats_30_fetching.set_value(false);
        }
        let Some(name) = stats_30_source.get() else {
            return;
        };
        if stats_30_fetching.get_value() || market.stats_30.with_untracked(Option::is_some) {
            return;
        }
        stats_30_fetching.set_value(true);
        let captured = Some(name.clone());
        leptos::task::spawn_local(async move {
            // A failed fetch stores the empty index on purpose: the cells
            // settle to "—" instead of shimmering forever, and the next
            // world change is what retries.
            let index = get_sale_stats(&name, STATS_30_WINDOW_DAYS)
                .await
                .map(|body| stats_index(&body))
                .unwrap_or_default();
            // Past the await the page may be gone and the world may have
            // moved: every touch is a `try_*`.
            if verdict(sell_world_name.try_get_untracked(), &captured) != Verdict::Proceed {
                return;
            }
            let _ = market.stats_30.try_set(Some(Arc::new(index)));
            let _ = stats_30_fetching.try_update_value(|f| *f = false);
        });
    });
```

and the table call gains `market=market`.

- [ ] **Step 5: The table publishes its rows and reads the handles**

`RecipeAnalyzerTable` gains the prop:

```rust
    /// The page-level handles E2's market columns use: the sparkline store
    /// the page's hook fills, the client-only 30-day body, the scroller's
    /// rendered range and the rows mirror the hook reads.
    market: MarketHandles,
```

After `computed_data` (`:2279`):

```rust
    // Publish the sorted rows for the page's lazy fetch — the hook reads
    // this mirror, so an empty mirror is no request at all. The clone is
    // one `Arc` per row and only happens while a lazy column is on.
    let wants_lazy =
        Memo::new(move |_| visible_cols.with(|v| v.contains(COL_TREND) || v.contains(COL_DRIFT)));
    Effect::new(move |_| {
        if wants_lazy.get() {
            market.rows.set(computed_data.get());
        } else if !market.rows.with_untracked(Vec::is_empty) {
            market.rows.set(Vec::new());
        }
    });
```

`computed_data` itself reads the 30-day body, so the table re-sorts when it lands:

```rust
        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = sort_dir().unwrap_or_else(|| mode.default_dir());
        // Reactive on the 30-day body: `None` until it lands (and forever
        // when no 30-day column asked for it), so this only re-runs when
        // something actually arrived.
        let stats_30 = market.stats_30.get();
        filter_and_sort(
            &priced(),
            &t,
            &world_names_for_rows,
            mode,
            dir,
            stats_30.as_deref(),
        )
```

`cell_ctx` hands both handles to the extractors:

```rust
    let cell_ctx = Signal::derive(move || CellCtx {
        now_unix: chrono::Utc::now().timestamp(),
        preview,
        // `with`, not `get`: this is read once per rendered row and `get`
        // would clone both sets each time.
        capped_cost: needs.with(|n| capped_flags(&n.capped)),
        // Copy handles: reading them costs nothing until a lazy cell
        // actually looks inside, inside the row's own closure.
        sparklines: Some(market.sparklines),
        stats_30: Some(market.stats_30),
    });
```

and the grid call uses the hoisted layout and forwards the range. **This task makes exactly two edits to that call** — the inline `layout=GridLayout { … }` becomes `layout=RECIPE_GRID`, and `visible_range=market.visible_range` is appended after `lab_columns` (which Task 1 already changed to `preview`). **Do not retype the surrounding props.** `header_class` and `row_min_width` come from the container-mode row-clip fix this branch stacks on (Global Constraints): dropping either one, or the `min-w-max` inside the header class, re-blanks every row below `md`, where this page's always-on columns alone are 768 px. At the branch base they read as constants, and `the_grid_call_opts_into_a_sized_row_spacer` (`recipe_analyzer.rs:3723`) `include_str!`s this file and asserts both by name:

```rust
                <AnalyzerGrid
                    columns=&RECIPE_COLUMNS
                    rows=computed_data
                    visible_cols=visible_cols
                    sort_mode=sort_mode
                    sort_dir=sort_dir
                    ctx=cell_ctx
                    custom=custom
                    layout=RECIPE_GRID
                    // From the row-clip fix — copy through unchanged, never
                    // reorder or reword. `RECIPE_HEADER_CLASS` carries the
                    // `min-w-max` that makes the header band span the
                    // scrolled width; `RECIPE_ROW_MIN_WIDTH` is what sizes
                    // the row spacer past the port width.
                    header_class=RECIPE_HEADER_CLASS
                    row_min_width=RECIPE_ROW_MIN_WIDTH
                    row_class=stripe
                    marks=marks
                    extras=header_extras
                    on_pill=on_pill
                    lab_columns=preview
                    visible_range=market.visible_range
                />
```

Read `recipe_analyzer.rs` as it then stands before touching this call and quote whatever form the props are in: at the branch base (`7baeaa71`) they are the two constants above; against the row-clip commit alone (`9f3ab4b4`) they are the literals `header_class="min-w-max flex flex-row align-top h-16 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]"` and `row_min_width="max-content"` with a six-line comment above them. Either way, only `layout=` changes and only `visible_range=` is added.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer`
Expected: PASS, 52 tests (48 + the four above), `the_grid_call_opts_into_a_sized_row_spacer` among them — it reads this page's `<AnalyzerGrid>` call back out of the source and fails if `header_class` or `row_min_width` stops naming its constant.

Run: `cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown`
Expected: exit 0 — the first check that the two client-only fetches and both `Effect` bodies compile for the browser. Run it with no `RUSTFLAGS` in the environment.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(recipe-analyzer): page-level sparkline store on the E1 hook, and the client-only 30-day body"
```

---
### Task 9: Header tooltips and the "7d · ‹world›" line, the Price median tell, Market and Location in the picker

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs`: `cell_price` (`:793-806`), two header-class consts beside `HEAD_LAZY_MD` (`:887-905`), the seven older `ColumnSpec` statics' `group` (`:691-730`), `window_and_place` + `market_extra` beside the other header helpers (`:2100-2130`), the `header_extras` memo's fallthrough (`:2188`), and three tests

**Interfaces:**
- Consumes: `HeaderExtra` / `HeaderLine2` with their new fields (Task 5), `PickerGroup::{Market, Location}` (Task 3), `CellNote::VsMedian` (Task 4), `delta_pct` (`:832`), the seven tooltip keys and `recipe_analyzer_window_7d` (Task 1).
- Produces:
  - `fn window_and_place(i18n, place: &str) -> String` — `"7d · ‹sell world›"`.
  - `fn market_extra(i18n, kind: ColumnKind, sell_place: &str) -> Option<HeaderExtra>` — the tooltip, the optional second line and the two-line classes for the seven market-side kinds; `None` for every other kind, so Phase D's four arms keep theirs.
  - `const HEAD_MD_2`, `const HEAD_28_MD_2` — the two-line classes Daily sales and Confidence use *only* while their extra is in effect.
  - `cell_price` returning `CellNote::VsMedian` when the sell world has a 7-day median to compare against.

- [ ] **Step 1: Write the failing tests**

```rust
    fn price_row(key: i32, price: i32, median: Option<i32>, fell_back: bool) -> RecipeRow {
        let mut r = Arc::try_unwrap(row(key, 0, 0, 1.0, 1)).ok().unwrap();
        r.market_price = price;
        r.rev_alt[PriceSignal::SaleMedian.index()] = median;
        r.revenue_fell_back = fell_back;
        Arc::new(r)
    }

    /// The Price note gains the signed percent the price sits above or
    /// below the sell world's
    /// 7-day median, keeps the listing tell in front of it when both apply,
    /// and is exactly the pre-Phase-C cell with the toggle off.
    #[test]
    fn the_price_note_carries_the_median_tell_under_the_toggle() {
        let key = fixture_recipes()[0].key_id.0;
        let ctx = test_ctx();
        let off = CellCtx {
            preview: false,
            ..test_ctx()
        };
        // A price of 138 against a median of 100 is 38% ABOVE it: positive,
        // and green. (The other orientation — the median measured against
        // the price — would paint a fake-low listing green.)
        assert_eq!(
            cell_price(&price_row(key, 138, Some(100), false), &ctx),
            CellValue::GilWithNote {
                amount: 138,
                note: CellNote::VsMedian {
                    listing: false,
                    pct: 38.0
                }
            }
        );
        // Below the median, and the listing tell keeps its place in front.
        assert_eq!(
            cell_price(&price_row(key, 75, Some(100), true), &ctx),
            CellValue::GilWithNote {
                amount: 75,
                note: CellNote::VsMedian {
                    listing: true,
                    pct: -25.0
                }
            }
        );
        // No sale history on the sell world: Phase D's note, unchanged.
        assert_eq!(
            cell_price(&price_row(key, 100, None, true), &ctx),
            CellValue::GilWithNote {
                amount: 100,
                note: CellNote::ListingFallback
            }
        );
        // Price IS the median (the median basis): no "+0%" tell.
        assert_eq!(
            cell_price(&price_row(key, 100, Some(100), false), &ctx),
            CellValue::GilWithNote {
                amount: 100,
                note: CellNote::None
            }
        );
        // Toggle off: no note line at all.
        assert_eq!(
            cell_price(&price_row(key, 138, Some(100), false), &off),
            CellValue::Gil(138)
        );
    }

    /// Every market column's header says what window it covers and where
    /// the number comes from; the two 30-day columns carry the window in
    /// their label instead, so they get a tooltip only.
    #[test]
    fn market_headers_carry_their_tooltip_and_the_window() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let daily = market_extra(i18n, ColumnKind::SalesPerDay7, "Gilgamesh").unwrap();
            let line2 = daily.line2.clone().expect("a second line");
            assert_eq!(line2.sub_label, "7d · Gilgamesh");
            assert!(line2.pill.is_none(), "no formula input to write");
            assert_eq!(daily.header_class, Some(HEAD_MD_2));
            assert!(!daily.title.is_empty());
            for kind in [
                ColumnKind::Confidence,
                ColumnKind::Trend,
                ColumnKind::DriftSpark,
            ] {
                let e = market_extra(i18n, kind, "Gilgamesh").unwrap();
                assert_eq!(e.line2.expect("a second line").sub_label, "7d · Gilgamesh");
            }
            assert_eq!(
                market_extra(i18n, ColumnKind::Confidence, "Gilgamesh")
                    .unwrap()
                    .header_class,
                Some(HEAD_28_MD_2)
            );
            for kind in [
                ColumnKind::ProfitPerDay,
                ColumnKind::VolumeUnits30,
                ColumnKind::Vwap30,
            ] {
                let e = market_extra(i18n, kind, "Gilgamesh").unwrap();
                assert!(e.line2.is_none(), "{kind:?}: the label carries the window");
                assert_eq!(e.header_class, None, "{kind:?}: classes do not move");
                assert!(!e.title.is_empty());
            }
            // Phase D's kinds keep their own extras; a plain column has none.
            assert!(market_extra(i18n, ColumnKind::HopGain, "Gilgamesh").is_none());
            assert!(market_extra(i18n, ColumnKind::Item, "Gilgamesh").is_none());
        });
    }

    /// Every optional column is in a named group, and the two new ones hold
    /// what the kit says they hold.
    #[test]
    fn the_grouped_picker_lists_market_and_location() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = use_i18n();
            let ctx = PickerContext {
                sell_place: "Gilgamesh".into(),
                buy_place: "Aether".into(),
                revenue: PriceSignal::ListingMin,
                cost: PriceSignal::ListingMin,
                capped: BTreeSet::new(),
            };
            let got = grouped_picker_options(&RECIPE_COLUMNS, i18n, &ctx);
            let mut headings: Vec<String> = got
                .iter()
                .map(|o| o.group.as_ref().expect("a heading").label.clone())
                .collect();
            headings.dedup();
            assert_eq!(
                headings,
                vec![
                    "Revenue · Gilgamesh",
                    "Cost · Aether",
                    "Travel",
                    "Market",
                    "Location"
                ]
            );
            let ids_in = |label: &str| -> Vec<&str> {
                got.iter()
                    .filter(|o| o.group.as_ref().unwrap().label == label)
                    .map(|o| o.id)
                    .collect()
            };
            assert_eq!(
                ids_in("Market"),
                [
                    "confidence",
                    "last-sold",
                    "volume",
                    "vwap",
                    "tax",
                    "profit-per-day",
                    "trend",
                    "drift",
                    "volume-30d",
                    "vwap-30d"
                ]
            );
            assert_eq!(ids_in("Location"), ["listing-world", "listing-dc"]);
        });
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer`
Expected: compile error — `cannot find function market_extra`, `cannot find value HEAD_MD_2`; then, once those exist, `the_grouped_picker_lists_market_and_location` fails on `["Revenue · Gilgamesh", "Cost · Aether", "Travel", "Other"]` and `the_price_note_carries_the_median_tell_under_the_toggle` fails with `CellNote::None` where `VsMedian` is expected.

- [ ] **Step 3: The Price median tell**

```rust
/// The Price slot. Under the toggle it carries an always-present note
/// sub-line: the listing tell when the price fell back to a listing, and
/// the signed percent the price sits above or below the sell world's
/// 7-day sale median — the
/// revenue-side answer to "is this listing-min price real?" (#1202). The
/// median is on the row already (`rev_alt`, filled from the body the page
/// always fetches), so the tell costs no request.
fn cell_price(r: &RecipeRow, ctx: &CellCtx) -> CellValue {
    if !ctx.preview {
        return CellValue::Gil(r.market_price);
    }
    let listing = r.revenue_fell_back;
    let median = r.rev_alt[PriceSignal::SaleMedian.index()];
    // `alt` = the price, `input` = the median: this sub-line sits under
    // Price and reads "this price is n% above/below the 7-day median" — the
    // opposite orientation from `rev_alt_cell`, where the alternative is
    // what the cell renders. `delta_pct` still yields `None` when the median
    // is unpriced *or* equal to the price, so the median basis never shows
    // itself "+0%".
    let note = match median.and_then(|m| delta_pct(Some(r.market_price), m)) {
        Some(pct) => CellNote::VsMedian { listing, pct },
        None if listing => CellNote::ListingFallback,
        None => CellNote::None,
    };
    CellValue::GilWithNote {
        amount: r.market_price,
        note,
    }
}
```

- [ ] **Step 4: The two two-line header classes and the seven extras**

Beside `HEAD_LAZY_MD`:

```rust
/// Daily sales and Confidence become two-line headers *only* while their
/// header extra is in effect (`HeaderExtra.header_class`): baking these
/// into the column table would move the toggle-off DOM. `md:flex`, not
/// `md:block` — `SortableHeaderCell` appends `flex flex-col justify-center`
/// for a two-line header and a later `md:block` would override it at md+.
const HEAD_MD_2: &str = "w-32 shrink-0 px-4 py-2 leading-tight hidden md:flex";
const HEAD_28_MD_2: &str = "w-28 shrink-0 px-4 py-2 leading-tight hidden md:flex";
```

and, beside the other header helpers:

```rust
/// A market column's second line: the window and where the number comes
/// from ("7d · Gilgamesh"), the kit's rule that a sub-label carries window
/// and source. The separator is the same one the signal columns use.
fn window_and_place(i18n: I18nContext<Locale, I18nKeys>, place: &str) -> String {
    format!("{} · {}", t_string!(i18n, recipe_analyzer_window_7d), place)
}

/// The header extra for a market-side column: its recipe-specific tooltip
/// (the flip finder's `analyzer_tooltip_*` describe 30-day resale-quality
/// numbers, which these are not), whether it carries the window line, and
/// the two-line classes the two pre-existing columns switch to while this
/// extra is in effect. `None` for every other kind, so Phase D's four arms
/// keep theirs.
fn market_extra(
    i18n: I18nContext<Locale, I18nKeys>,
    kind: ColumnKind,
    sell_place: &str,
) -> Option<HeaderExtra> {
    let (title, windowed, header_class) = match kind {
        ColumnKind::SalesPerDay7 => (
            t_string!(i18n, recipe_analyzer_tooltip_daily_sales),
            true,
            Some(HEAD_MD_2),
        ),
        ColumnKind::Confidence => (
            t_string!(i18n, recipe_analyzer_tooltip_confidence),
            true,
            Some(HEAD_28_MD_2),
        ),
        ColumnKind::ProfitPerDay => (
            t_string!(i18n, recipe_analyzer_tooltip_profit_per_day),
            false,
            None,
        ),
        ColumnKind::Trend => (t_string!(i18n, recipe_analyzer_tooltip_trend), true, None),
        ColumnKind::DriftSpark => (t_string!(i18n, recipe_analyzer_tooltip_drift), true, None),
        // The 30-day pair says its window in its label, so line 2 would
        // only repeat it.
        ColumnKind::VolumeUnits30 => (
            t_string!(i18n, recipe_analyzer_tooltip_volume_30d),
            false,
            None,
        ),
        ColumnKind::Vwap30 => (
            t_string!(i18n, recipe_analyzer_tooltip_vwap_30d),
            false,
            None,
        ),
        _ => return None,
    };
    Some(HeaderExtra {
        title: title.to_string(),
        line2: windowed.then(|| HeaderLine2 {
            sub_label: window_and_place(i18n, sell_place),
            pill: None,
        }),
        header_class,
    })
}
```

(each `t_string!` here is a plain key, so the tuple holds `&'static str` and `.to_string()` happens once, at the end.)

The `header_extras` memo's fallthrough becomes the market arm:

```rust
                kind => match market_extra(i18n, kind, &sell_place.get()) {
                    Some(extra) => extra,
                    None => continue,
                },
```

- [ ] **Step 5: The seven older columns move to Market and Location**

In the `ColumnSpec` statics (`:691-730`), `group: PickerGroup::Other` becomes `PickerGroup::Market` on `SPEC_CONFIDENCE`, `SPEC_LAST_SOLD`, `SPEC_VOLUME`, `SPEC_VWAP` and `SPEC_TAX`, and `PickerGroup::Location` on `SPEC_WORLD` and `SPEC_DC`. Every other spec keeps `Other`: none of them has a `?cols=` token, so the picker never shows that group.

The flat picker (toggle off) is unaffected — it lists ids, not groups, and `picker_options` still filters `lab.is_none()`.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p ultros-app --lib -- routes::recipe_analyzer`
Expected: PASS, 55 tests (52 + the three above). `picker_columns_are_a_subset_of_optional_column_order` still passes (it counts ids, not groups).

Run: `cargo test -p ultros-app --lib -- analyzer_kit`
Expected: PASS, 66 tests.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(recipe-analyzer): market header tooltips and 7d sub-labels, the Price vs-median tell, Market/Location picker groups"
```

---
### Task 10: Changelog, e2e, the spec's two stale sentences, the CI gate, the measurement and the PR

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/changelog.rs:33-40` (one entry on top)
- Modify: `integration/runner.cjs` (the lab route gains the five new `?cols=` tokens, in both places)
- Modify: `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md` (§6's SSR row count, §11's token list)
- Create (scratchpad, not committed): `…/scratchpad/phase-e2/phase-e2-pr-body.md`

**Interfaces:**
- Consumes: everything above.
- Produces: a green `./check_ci.sh`, the measured sparklines POST, the PR.

- [ ] **Step 1: The changelog entry**

At the top of `CHANGELOG` (newest first; equal dates are allowed):

```rust
    ChangelogEntry {
        date: "2026-09-03",
        title: "Recipe Analyzer: one Labs toggle, plus Profit/day, a price trend, its drift, and 30-day volume and VWAP",
        blurb: "The Recipe Analyzer's two Labs toggles are now one, \"Recipe Analyzer: the market model\" under Settings › Labs — turn it back on there if you had either of the old ones. It carries everything they did, plus five new columns in the Columns picker: Profit/day (profit times how fast the item sells), Trend (the last 7 days of prices as a sparkline), Drift (how far that trend moved), and Volume (30d) and VWAP (30d) for a longer view than the 7-day pair. Trend and Drift load only for the rows you can see; the 30-day columns load their own data the first time you show one. Price now says how it compares with the sell world's 7-day median, and the Columns picker groups everything under Market and Location.",
        link: Some("/settings"),
    },
```

The two older entries (Phase C's and Phase D's) keep their wording: they are the record of what shipped that day, and this entry is where a reader learns the toggle names changed.

- [ ] **Step 2: The e2e route shows the new columns**

In `integration/runner.cjs`, the single `?labs=analyzer-recipe` route (Task 1) gains the five tokens, in the assertions map and in `getRoutes()`:

```
/recipe-analyzer?world=Gilgamesh&labs=analyzer-recipe&cols=confidence,cost-sale-median,rev-sale-median,hop-gain,hop-worlds,profit-per-day,trend,drift,volume-30d,vwap-30d
```

The sweep's real value here is the console-error and horizontal-overflow checks with every optional column on at once: the market columns are `hidden md:*`, so the mobile pass renders none of them and the desktop pass renders all fifteen.

- [ ] **Step 3: Two stale sentences in the kit spec**

Docs-only, in `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md`:

1. §6 "Lazy" (find `20 rows on SSR`): "the flip finder is Window mode, 20 rows on SSR and about 32 at 1080p, so 92 keys" becomes "the flip finder is Window mode, 28 rows on SSR (the 20-row fallback plus overscan 8) and about 32 at 1080p, so 88 to 92 keys" — what `flip_window_is_one_request_below_the_derived_threshold` actually asserts (E1 review, note 6).
2. §11 (find `analyzer-market-columns`): the per-phase token list becomes "Each experiment is a `&'static str` token. The recipe analyzer has exactly one, `analyzer-recipe`: Phase E2 merged Phase C's `analyzer-ledger` and Phase D's `analyzer-signal-columns` into it (one tool, one toggle — separate flags per phase made 'which permutation is this?' a question), and Phase F's sell scope ships under the same token."

- [ ] **Step 4: fmt, tests, clippy, wasm**

```bash
cargo fmt --all
cargo test -p ultros-app --lib > /tmp/tests.log 2>&1; echo "REAL_EXIT=$?"; tail -5 /tmp/tests.log
cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown > /tmp/wasm.log 2>&1; echo "REAL_EXIT=$?"; tail -5 /tmp/wasm.log
export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: every `REAL_EXIT=0`. Clippy runs `--all-targets` with `-D warnings`; fix in place, never `#[allow]`. Likely candidates from this phase, each with its fix:

- **Dead code** on anything Task 3 added whose consumer landed in a later task — by Task 9 every one has a production reader (`LazyFeed::hours` ← the fetch, `Enrich::{map, is_loading}` ← the extractors and `render_cell`, `SparkStore`/`SparkValue` ← the store and the fetch, `Enrichment::state` ← `spark_with`, `LateStats`/`stat_row_either` ← `late_30`, `stat_30` and `price_rows`, `PickerGroup::{Market, Location}` ← the specs, the five `ColumnKind`s ← the specs and `market_extra`, `HeaderExtra.header_class` ← `header_cell`). If clippy still names one, the wiring is missing, not the lint.
- `clippy::too_many_arguments` — `filter_and_sort` now takes six and `header_cell` seven; both are at or under the limit. Do not add a seventh to the hook.
- `clippy::type_complexity` — `Vec<(SparkKey, SparkValue)>` and `Option<RwSignal<SparkStore>>` are aliases deep enough to stay under the threshold; if a signature spells the tuple out, use `SparkKey`.
- `clippy::unnecessary_map_or` if `is_none_or` was written as `map_or(true, …)`.
- `unused_imports` if Task 8's `chunk_keys` / `rows_for_viewport` were imported at module level instead of inside `mod tests`: they have no non-test caller, and `--all-targets` compiles the lib without `cfg(test)`.
- `clippy::redundant_closure` on `spark_with`'s `.map(f)` if it was written `.map(|v| f(v))`.
- **E0382** (a rustc error, not a lint) in `spark_entry` if `points: s.points` is written before the key or before `delta_pct`: an assignment evaluates its value before its place, and `points` moves `s`.
- If clippy is OOM-killed (exit 137, `Killed: 9`), re-run `cargo clippy --all-targets -j 2 -- -D warnings`.

Commit any fixes with `git commit -am "chore(phase-e2): fmt and clippy"`.

- [ ] **Step 5: Measure the sparklines POST for a recipe-sized window**

The one measurement this phase owes (kit §8's "Trend and Drift ship only if Phase 0's numbers are acceptable"; the recentSales and 30-day bodies are already measured in the Decisions table). Read-only GET + POST against prod:

```bash
curl -s --compressed https://ultros.app/api/v1/cheapest/Gilgamesh -o /tmp/cheap.json
python -c "
import json
d = json.load(open('/tmp/cheap.json'))
ids = [[x['item_id'], False] for x in d['cheapest_listings'][:79]]
open('/tmp/spark.json', 'w').write(json.dumps({'items': ids, 'hours': 168}))
print(len(ids), 'keys')
"
curl -s --compressed -H 'Content-Type: application/json' --data @/tmp/spark.json \
  -o /tmp/spark.out -w 'wire %{size_download} B, %{time_total} s\n' \
  https://ultros.app/api/v1/sparklines/Gilgamesh
wc -c /tmp/spark.out
```

Record the wire bytes, the raw bytes (`wc -c`, since `--compressed` decompresses into the file) and the time in the PR body. 79 keys is exactly one visible-window batch (`the_recipe_window_is_one_request_per_scroll_settle`).

- [ ] **Step 6: The PR body**

Write `phase-e2-pr-body.md` in the scratchpad, substituting `<N>` with the `test result: ok. N passed` total from `/tmp/tests.log` and `<BYTES>` / `<MS>` with Step 5's numbers — the only placeholders in this plan:

```markdown
# Analyzer kit phase E2: the market columns, and one Labs toggle for the recipe analyzer

**Base branch: `main`.** Part of #1233. Plan: `docs/superpowers/plans/2026-09-03-analyzer-kit-phase-e2-market-columns.md`. This branch stacks on the container-mode row-clip fix, so that commit is in the diff; nothing here re-applies or re-derives it, and the recipe analyzer's `min-w-max` header band and `max-content` row spacer are its, not E2's.

## What's in it

- **One toggle.** `analyzer-recipe` replaces `analyzer-ledger` (Phase C) and `analyzer-signal-columns` (Phase D): one Labs entry, one description, one signal threaded through the page and the table. Aaron's call — "multiple permutations of the feature is a little much". **The two old tokens are gone, not aliased:** a stored `LABS` cookie or a bookmarked `?labs=analyzer-ledger` parses to the empty set (`Labs::from_str` drops unknown tokens), so anyone testing re-toggles once in Settings. It also frees the flag budget: Phase F's sell scope ships under this same toggle.
- **Five columns**, all default off, all `hidden md:`, appended to `?cols=` after the seventeen existing tokens: `profit-per-day` (profit × the 7-day rollup rate, computed, sortable), `trend` (a 168-hour sparkline, lazily fetched for the visible window, never sortable), `drift` (that series' first-to-last percent, same request, never sortable), `volume-30d` and `vwap-30d` (a client-only 30-day `sale_stats` body, sortable, fetched only when one of them is visible or the sort target). `?sort=` gains three tokens, `SortMode` three variants: 22 optional ids, 24 sort modes.
- **Drift is the spec's downgrade, deliberately.** The body-backed Drift (kit §8, decision point 3) would put 9,035,358 B raw / 1,170,030 B on the wire into the client for one optional column (measured on prod, 2026-09-03). The sparkline delta is the same question answered from a request the Trend column already makes.
- **The lazy layer in the kit**: `Layer::Lazy(LazyFeed::Sparklines { hours })`, `Sortability::LazyNever` (a lazy column is unreachable from a `?sort=` token even if given one), `Enrich<V> { Loading, Missing, Ready }`, `Enrichment::state`, `SparkStore`, four `CellValue`s that keep one element shape across their three states, `LateStats` + `stat_row_either`, and an `AnalyzerGrid` `visible_range` prop (promised by Phase B, deferred twice). `AnalyzerRow::enrich_key`, which the kit spec's §8 ledger lists under E2, is deliberately **not** added: `use_visible_enrichment` takes `key_of: fn(&T) -> K`, so a page-side `recipe_spark_key` does the job with no kit surface and nothing dead, and a defaulted trait method returning `Option<(i32, bool)>` would not fit a page whose key is a different shape.
- **The hook runs at page level** over a rows mirror the table publishes, so a cost-basis switch — which remounts the table — keeps every settled key, and only a sell-world change resets the store. The mirror stays empty unless Trend or Drift is visible, so the toggle-off page issues no request.
- **Headers and Price**: Daily sales, Confidence, Trend and Drift carry "7d · ‹sell world›" and a recipe-specific tooltip (the flip finder's tooltips describe 30-day resale-quality numbers, which these are not); Price's note line now reads `‹listing · ›vs median ±n%` against the sell world's 7-day median — zero fetch, the number is already on the row. The Columns picker groups everything under Revenue, Cost, Travel, **Market** and **Location**.
- **Numbers: none on existing columns.** Two pure refactors touch existing arithmetic — `profit_per_day` delegates to `profit_per_day_from_rate`, and the pricing pass's sell-stat lookup becomes `stat_row_either` — and both are pinned by tests that did not change, including the recorded pricing oracle.
- **The flip finder changes in exactly one line**: its Drift cell's colour class comes from `analysis::signed_delta_class`, which is byte-identical for every value the old arms produced (`+{d:.0}%` and `{d:.0}%` are `{d:+.0}%` over the ranges they guarded).

## Capacity

| Body | When | Raw | Wire |
|---|---|---|---|
| `sale_stats/{world}?window=30` | a 30-day column is visible or sorted | 3,250,000 B | 437,759 B |
| `sparklines/{world}` POST, 79 keys, 168 h | per scroll settle, Trend or Drift on | <RAW> B | <BYTES> B in <MS> |
| `recentSales/{world}` | **not fetched** (the downgrade avoids it) | 9,035,358 B | 1,170,030 B |

Watch `ultros_sale_stats_cache_total{disposition=loaded}` after this deploys: the 30-day columns add up to one cache key per sell world.

## Verification

- `cargo test -p ultros-app --lib`: <N> passed.
- `cargo check … --features hydrate --target wasm32-unknown-unknown`: exit 0.
- `./check_ci.sh`: `REAL_EXIT=0`, no `#[allow]` added.
- Toggle-off identity, by construction and by test: `BASE_COLUMN_ORDER` (no lab token), `SortMode::lab_only` (13 modes dropped), `lab_columns=false` (lab columns dropped from the header at build time, so no `<!>` marker), the flat picker, an empty `HeaderExtras`, `CellCtx.preview = false`, and both new fetches gated on tokens that cannot be visible or sorted with the toggle off.

## Manual checks (reviewer step, on prod after deploy)

A local network pass cannot prove any of the lazy behaviour: local enrichment never fires on the analyzer pages (no ClickHouse-backed data behind the dev server), so the checks below are prod-or-nothing.

1. `/recipe-analyzer?world=Gilgamesh` with the toggle **off**: identical to the pre-Phase-C page apart from the row-clip fix's header band and row min-width, which land in their own PR — same columns, same numbers, and DevTools Network shows `cheapest`, `cheapest` (sell world) and one `sale_stats?window=7`, nothing else.
2. Turn the toggle on in Settings › Labs, then show Trend and Drift: one POST to `/api/v1/sparklines/Gilgamesh` with `hours: 168` and ~79 items; the cells go skeleton → sparkline / signed percent; a row with no 7-day trades shows the empty sparkline and "—" with the "not enough sales" tooltip.
3. Scroll one screen and stop: exactly one new POST, and the union of `items` across requests has no repeats. Scroll back up: no request at all (the store keeps settled keys).
4. Switch the cost basis (Market → cost basis → sale median): the table remounts and re-prices, and **no** new sparklines POST goes out for rows already fetched — the point of the page-level store.
5. Switch the sell world: skeletons come back and a fresh POST goes out; no row shows the previous world's series.
6. Show Volume (30d): one `GET /api/v1/sale_stats/Gilgamesh?window=30`; the two cells shimmer, then fill. Hide and re-show the column: no second request. Sort by it before it lands (`?sort=volume-30d` on a cold load): the table shows profit order, then re-sorts itself when the body arrives.
7. Price's sub-line reads `vs median ±n%` (green above +1%, red below −1%, muted inside), and `listing · vs median …` on a row whose price fell back to a listing.
8. Daily sales and Confidence headers read two lines ("7d · Gilgamesh") at md+ and are unchanged below it; hovering any market header shows its tooltip.
9. `/flip-finder/Gilgamesh`: the Drift column is identical to prod — same text, same colours, same dead band.
10. `./scripts/run_e2e.sh` (desktop + mobile, `STRICT_CONSOLE` on).
```

- [ ] **Step 7: Push and open the PR**

```bash
git push -u origin claude/issue-1233-phase-e2-market-columns
gh pr create --base main --title "Analyzer kit phase E2: market columns, and one Labs toggle for the recipe analyzer" --body-file "<scratchpad>/phase-e2/phase-e2-pr-body.md"
```

If `main` has moved: `git fetch origin && git rebase origin/main`, re-run Step 4, `git push --force-with-lease`.

---
## Self-review (done while writing; kept for the executor)

**Spec coverage — kit §8's Phase E2 paragraph, bullet by bullet.** *Profit/day (computed, default off)* → Task 7's `cell_profit_per_day` + `SortMode::ProfitPerDay`, `Layer::Computed`, `default_on: false`, pinned by `profit_per_day_is_profit_times_the_rollup_rate`. *Trend (lazy)* → Tasks 3 (`Layer::Lazy`, `SparkStore`), 4 (`CellValue::Sparkline`), 7 (the row) and 8 (the hook at page level), pinned by `the_recipe_window_is_one_request_per_scroll_settle` and `lazy_cells_keep_one_shape_per_variant`. *Drift (recentSales body, needed-gated; `drift_needed` on the folded key; the `(item, hq)` buffer index)* → **deliberately downgraded** to the spec's own fallback (§5 L292, decision point 3's "if Phase 0's size is bad"), because Phase 0's measurement came in at 9.0 MB raw / 1.17 MB wire: Drift reads the sparkline feed's first-to-last percent, so there is no `drift_needed`, no `raw_sales_key` change and no buffer index. First row of the Decisions table, and a bullet in the PR body. *Volume 30d and VWAP 30d (client-only 30d body)* → Tasks 6 (the gate), 7 (rows, comparators, the fallback sort) and 8 (the Effect), pinned by `thirty_day_columns_need_a_second_sell_world_body`, `thirty_day_sorts_fall_back_to_profit_until_the_body_lands` and `the_thirty_day_body_is_only_requested_when_a_30d_column_is`. *Tooltips and "· 7d" sub-labels on Sales/day and Confidence* → Task 9's `market_extra`, with `HeaderExtra.header_class` so their classes move only under the toggle. *The signed Price sub-line vs the 7d median* → Task 4's `CellNote::VsMedian` + Task 9's `cell_price`. *Market and Location picker groups* → Tasks 3 and 9, pinned by `the_grouped_picker_lists_market_and_location`. *Numbers: none on existing columns* → a Global Constraint, with the oracle and the three `profit_per_day_*` tests as the proof. *Changelog* → Task 10. §9's URL rules: five tokens appended after the seventeen, three sort tokens (none for `trend` or `drift`), no new selection key, `migrate_legacy_params` untouched, every key in seven locales. §6's Lazy rules: the E1 hook unchanged, the store at page level, the window derived from `rows_for_viewport` rather than a literal. §10 decisions 3, 4, 5 and 7: all four answered in the Decisions table (downgrade, opt-in, default-off-and-watch, `hidden md:` through F), with decision 7's mechanics also spelled out in Global Constraints.

**Where this plan overrides the spec, and why.** (1) §11's per-phase tokens: one `analyzer-recipe` token replaces C's and D's and absorbs E2 and F, on Aaron's instruction; the spec sentence is corrected in Task 10, and the retired tokens' loss of function is stated in the Decisions table, the changelog entry and the PR body. (2) The variant ledger (§8 L373–379) lists `AnalyzerRow::enrich_key` among E2's items: this plan does **not** add it. The hook takes `key_of: fn(&T) -> K`, so a page-side `recipe_spark_key` (exactly like the flip finder's `flip_key`) does the job with no kit surface and nothing dead; a defaulted trait method with one caller on one page would be a promise the kit cannot keep for a page whose key is not `(i32, bool)`. Declared here and in the PR body. (3) `CellValue` grows four variants where §3 sketched two: `Sparkline` carries `SparkValue` rather than `(Arc<[u32]>, f32)` (the component takes a `Vec`, and the colour driver is genuinely optional), `LazyPct` backs Drift, `LateGilWithPct` backs VWAP 30d, and `LateCount` is §3's `LazyCount` widened to `u64` (`ItemSaleStats.units_sold` is one) and renamed because that body is late, not lazy — Phase G's flip-finder port folds its own count into `LateCount` rather than adding a second kind. (The shipped `cells.rs` already departs from §3's enum in about ten ways; it is a sketch, not a contract.) (4) `ColumnKind::DriftSpark`, not `DriftBuffer`: kinds name definitions, and the buffer-backed drift the flip finder shows is a different number from a different body.

**Placeholder scan.** No "TBD", no "similar to Task N", no "…and so on". Every step shows the code it adds, every test step shows the test, every `Run:` has an `Expected:`. The only placeholders are in Task 10's PR body — `<N>` (the test total), `<BYTES>` / `<RAW>` / `<MS>` (Step 5's measurement) — and Task 10 says exactly where each comes from. One block is deliberately elided rather than placeheld: Task 5's `header_cell` rewrite marks the marked-header arm `/* grid.rs:169-189, verbatim */`, with an instruction above it to keep those lines character for character; the same task's Task 8 sibling tells the executor to read the live `<AnalyzerGrid>` call rather than paste a form that may have moved.

**Type consistency across tasks.** `SparkKey = (i32, bool)` is what `recipe_spark_key` returns (Task 8), what `SparkStore = Enrichment<SparkKey, SparkValue>` is keyed on (Task 3), what `spark_with` builds from `(r.recipe.item_result, r.stat_hq)` (Task 7) and what `spark_entry` produces from `(s.item_id, s.hq)` (Task 8) — the hook's `K: Copy + Eq + Hash + Send + Sync + 'static` holds for it. `SparkValue: Absorb + Send + Sync + Clone + Debug + PartialEq` satisfies both the hook's `V` bound and `CellValue`'s derives (`Enrich<SparkValue>` inherits them; `Eq` is *not* derived on `Enrich`, which is why `CellNote` also drops `Eq` in Task 4). `S = Option<String>` from `sell_world_name: Memo<Option<String>>` matches `fetch_recipe_sparklines(Option<String>, Vec<SparkKey>)` and the hook's `S: Clone + PartialEq + Send + Sync`, and it is what `verdict(sell_world_name.try_get_untracked(), &captured)` compares in the 30-day Effect (`T = Option<String>` on both sides). `LateStats = RwSignal<Option<Arc<StatsIndex>>>` is the type of `MarketHandles.stats_30`, of `CellCtx.stats_30`'s payload, and of what `market.stats_30.get()` hands `filter_and_sort` as `Option<&StatsIndex>` via `as_deref()`. `stat_row_either(&StatsIndex, i32, bool) -> Option<&ItemSaleStats>` serves `price_rows` (Task 7), `late_30` (Task 7) and `stat_30` (Task 7) with the same three arguments. `Enrich::map`'s `impl FnOnce(V) -> U` accepts `SparkValue::clone` and `|v| v.delta_pct` over `Enrich<&SparkValue>`. `CellCtx` stays `Copy + Clone + Debug + PartialEq + Eq` with two `Option<RwSignal<…>>` fields (`RwSignal` is `Copy`/`Eq`/`Hash` for any `T` and `Debug` for any `T` given `S: Debug`). `GridLayout` is `Copy`, so `RECIPE_GRID` is a `const` usable both in the `view!` and in the window test. `EnrichmentConfig` is `Copy`, so `RECIPE_ENRICHMENT` is a `const` the effect captures by value. `filter_and_sort` has six parameters and `header_cell` seven — at the `too_many_arguments` limit, not over it, and the hook keeps its seven.

**The review traps, one by one.** (1) *`#[prop(optional)]` strips `Option`*: the grid's `visible_range` prop is declared `Option<RwSignal<(usize, usize)>>` and the page passes a bare `RwSignal`; because the *scroller's* prop strips it the same way, an `Option` cannot be forwarded, so the grid substitutes a signal of its own — stated in the Decisions table and pinned by `visible_range_is_optional_and_changes_no_markup`. (2) *A hidden optional column still writes a `<!>` marker*: `lab_columns` stays a plain bool and still drops lab columns at build time (one token gates all fifteen), so Phase D's marker test keeps its meaning; no new arm renders an `Option` child — every lazy cell renders both its skeleton and its value slot in every state and toggles classes. (3) *A `fn` item cannot unsize into `&'a dyn Fn(..) -> &'a str`*: nothing new is `dyn`; `key_of` is a `fn` pointer, `market_extra` returns owned `String`s, and the only `dyn` in play (`CustomCell`) is untouched. (4) *Plain-key `t_string!` is `&'static str`*: `market_extra` holds its seven tooltips as `&'static str` and calls `.to_string()` once at the end; `analyzer_price_vs_median` is the one interpolated key and is `.to_string()`d at its builder. No `&t_string!(..)` appears. (5) *Missing locale keys only warn*: Task 1 Step 8's `python -c len(d)` (1790 per locale) and `grep -c` are the gate, and the same step greps that the only surviving mention of either retired token is the labs test that asserts they are dead. The script also asserts, per locale, that the new `recipe_analyzer_col_drift` differs from `analyzer_col_spark` — fr and de translate the flip finder's two keys identically, which is why this column has its own. (6) *`type_complexity` on tuple slices*: `SparkKey`, `SparkStore`, `LateStats` and `MarketHandles` keep every signature short. (7) *E0382 in a zip-style map*: `spark_entry` binds `(s.item_id, s.hq)` and `delta_pct` before `points: s.points` moves `s`, and Task 10 names it as the likely compile error. (8) *Disposed-signal reads after an await*: the 30-day Effect's post-await path is `verdict(...try_get_untracked(), &captured)`, `try_set`, `try_update_value`, and the sparkline path is the E1 hook's, unchanged. (9) *A store read inside the row closure re-renders the row*: accepted by the spec (L212–214) and stated in the Decisions table; the mirror `Effect` clones one `Arc` per row and only while a lazy column is on.

**Every new `pub` item has a non-test reader inside this PR.** Kit: `LazyFeed` and `hours()` ← `RECIPE_TREND_FEED` and `fetch_recipe_sparklines`; `Layer::Lazy` ← the Trend and Drift rows; `Sortability::LazyNever` ← `sortability_for`, `sort_from_token`, `header_cell`; the five `ColumnKind`s ← the five `ColumnSpec` statics and `market_extra`; `PickerGroup::{Market, Location}` ← the twelve specs that name them and `heading`; `CellCtx.preview` ← `cell_price`; `CellCtx.sparklines` / `stats_30` ← `spark_with` / `late_30`; `Enrich` + `map` + `is_loading` ← the extractors and four `render_cell` arms; `SparkKey` / `SparkValue` / `SparkStore` ← `MarketHandles`, the fetch, the extractors; `Enrichment::state` ← `spark_with`; `LateStats` ← `CellCtx` and `MarketHandles`; `stat_row_either` ← `price_rows`, `late_30`, `stat_30`; the four new `CellValue`s ← the five extractors; `CellNote::VsMedian` ← `cell_price`; `HeaderLine2.pill` / `HeaderExtra.header_class` ← `header_cell` and `market_extra`; the grid's `visible_range` ← the recipe grid call. `analysis`: `profit_per_day_from_rate` ← `profit_per_day`, the cell and the comparator; `DELTA_DEAD_BAND_PCT` and `signed_delta_class` ← the flip finder's Drift cell, the `LazyPct` arm and the `GilWithNote` arm; `first_to_last_pct` ← `spark_entry`. `needed`: `STATS_30_WINDOW_DAYS` and `RecipeNeeds.stats_30` ← `needed_bodies` and `stats_30_key`. Page-private items (`MarketHandles` and all four fields, `recipe_spark_key`, `spark_entry`, `fetch_recipe_sparklines`, `RECIPE_ENRICHMENT`, `RECIPE_GRID`, `RECIPE_TREND_FEED`, `stats_30_wanted`, `stats_30_key`, `effective_sort_mode`, `stat_30`, `spark_with`, `late_30`, `market_extra`, `window_and_place`, the five cells, five labels, five specs, five tokens, `HEAD_MD_2`, `HEAD_28_MD_2`, `HEAD_LAZY_MD`, `HEAD_LAZY_MD_END`, `CELL_28_MID_MD`, `CELL_28_NUM_MD`, `RecipeProfitData.stat_hq`) are each read by the table, the page or the `view!`. Items that are dead **between** tasks are called out where they appear; Task 10's `check_ci.sh` is the branch-level gate.

**Toggle-off identity — now one condition, not four.** Collapsing C's and D's flags means there is a single off state to pin, and it is "the page as it was before Phase C", minus the one carve-out Global Constraints names: the row-clip fix's `min-w-max` header band and `max-content` row spacer are a DOM change that fix makes for every user, in its own PR, and E2 neither adds to nor removes them. Mechanisms and their tests: the `?cols=` contract (`recipe_optional_column_order_is_a_stable_url_contract` — `BASE_COLUMN_ORDER` is still exactly the seven Phase B tokens); the sort gate (`lab_only_sort_modes_are_exactly_the_thirteen` plus the page's `sort_mode` filter); the header (`lab_columns_are_absent_from_the_header_unless_enabled`, unchanged in meaning because one token gates every lab column); the pickers (`picker_columns_are_a_subset_of_optional_column_order`'s flat assertion, `picker_options` filtering `lab.is_none()`); the cells (`the_price_note_carries_the_median_tell_under_the_toggle`'s off case, and `render_cell`'s untouched arms — `the_price_note_adds_the_median_tell_without_moving_phase_d` asserts the two pre-existing notes still render `class="{SUB_LINE}"`); the headers (`header_extras_render_title_sub_label_and_pill`'s "empty extras map is the flag-off path" and `unsortable_headers_take_a_title_and_a_second_line`'s equality assertion); the fetches (`the_thirty_day_body_is_only_requested_when_a_30d_column_is` over both contracts, and the rows mirror that stays empty unless Trend or Drift is visible). The one deliberate difference in the off state: `?labs=analyzer-ledger` and `?labs=analyzer-signal-columns` no longer turn anything on.

**The flag budget.** `LABS` ends with exactly one entry, so `the_experiment_list_stays_short` (≤ 3) has two slots free and **Phase F needs no retirement** — its sell scope ships under `analyzer-recipe`. The removal rule still applies to the one flag: it is deleted, and the market model becomes the default, in the phase after Aaron has validated it on prod.

**Not in this plan, by decision:** `LazyFeed::ResaleQuality`, `CellValue::{LazyConfidence, Cadence}` and `GridLayout { hscroll }` (Phase G, with the flip finder's port; §3's `LazyCount` is superseded by `LateCount` above); `AnalyzerRow::enrich_key` (a page-side `fn` does it, see above); `ColumnSpec.canonical_id` and the `CATALOG` (G); a Drift **filter** chip or floor (kit §12 leaves hop and drift chips out); the other five `signed_delta_class` copies (market pulse, recently viewed, movers, trends, related items — different dead bands and decimals, G/H); the sell scope and `scope-vs-home` (F); listing age (J); a retry for a failed 30-day body (a sell-world change is the retry, stated in the Decisions table); and Phase D's owed `<!>` marker-delta bisect, which is not this branch's to close.
