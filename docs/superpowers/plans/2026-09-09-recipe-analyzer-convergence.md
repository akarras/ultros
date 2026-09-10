# Recipe Analyzer convergence — migration plan

Spec: `docs/superpowers/specs/2026-09-09-recipe-analyzer-convergence-design.md`.
Closes #1331; completes T08 of #1351. Order matters: each step compiles and
passes on its own.

## 1. `ultros-calc/src/needed.rs` — window-aware body set

- [x] `RecipeNeeds { window: u16, rev_gil: bool, cost_gil: bool, .. }`, manual
      `Default` with `window = SALE_STATS_WINDOW_DAYS`.
- [x] `needed_bodies`: `BuyScopeStats(needs.window)` when a sale cost signal,
      a sale `cost_signals` entry, or `cost_gil` wants it (alias rule unchanged);
      `SellWorldStats(needs.window)` when `window != 7` and the sell world is the
      revenue place with a sale revenue want, or an aliased buy side wants sale
      stats; `SellScopeStats(needs.window)` at a wider scope with the same
      triggers; `SellWorldStats(30)` for `stats_30`.
- [x] Tests: default set unchanged at window 7; window 30 produces `(30)` bodies
      plus the 7-day context body; alias per window; gil triggers.

## 2. `analyzer_kit/market.rs` — supplied slots

- [x] `use_market_data_with(scope, window: MarketWindow, provided: Signal<Vec<Window>>)`;
      `use_market_data` delegates with an empty `provided`.
- [x] `MarketData::supply(window, scope, stats, failed)` writes a slot the loader
      leaves alone; `fetch_stats` skips windows in `provided`, keeps a matching
      body when a window stops being provided.
- [x] `pub fn want(window)` for page-declared column needs.
- [x] Tests: a provided window never fetches; `supply` serves `stats()`; a scope
      change hides a supplied body.

## 3. `analyzer_kit/grid.rs` + `columns.rs` — host and kinds

- [x] `AnalyzerGrid` takes `market` and `subject`, renders `MarketGrid`.
- [x] `ColumnKind::RevGil`, `ColumnKind::CostGil`.
- [x] Grid tests construct a `MarketData` with `use_market_data`.

## 4. `routes/recipe_analyzer.rs` — the page

- [x] `MarketWindow` on the page; `MarketWindowControl` in `RecipePriceControls`.
- [x] `sell_market = use_market_data_with(sell_world, window, provided_windows)`.
- [x] Resource keys carry the window: `buy_sale_stats_scope`, `sell_scope_source`,
      new `sell_window_stats`. `fetch_sell_history` stays 7d.
- [x] Delete `stats_30_key`, the `stats_30_source` effect trio and
      `MarketHandles::{stats_30, stats_30_unavailable}`; `stats_30_wanted` →
      `sell_market.want(D30)`; `CellCtx` handles mirrored from the D30 slot;
      `filter_and_sort` reads the slot.
- [x] Table: resolve the revenue-side statistics per window (the new sell-world
      window body when present), `supply` the 7-day and window bodies.
- [x] Labels: `signal_label` / `short_signal` / `signal_help` /
      `cost_basis_options` take the window; `window_and_place` takes a `Window`.
- [x] `rev-gil` / `cost-gil`: specs, cells, sort modes, comparator, row fields,
      `price_rows` computation, picker groups, header extras, tooltips.
- [x] Filters: `register_filters` at page level; delete the chip markup,
      `ADDABLE_FILTERS`, `active_filters`, `pending_filter`, `clear_all`,
      `Thresholds` predicates; `column_filters` keeps only calculation controls.
- [x] Picker: shared `market_picker_options(window)` appended; toggles preserve
      foreign ids; `MarketSubject` from the row.
- [x] Tests updated for the new contracts (23 → 25 ids, 25 → 27 sort modes,
      registry keys, window labels, gil rules, needle guards).

## 5. Locales (seven files)

- [x] Windowed variants of the short signal names and the basis help lines;
      labels and tooltips for the two gil columns; remove the now-unused
      `recipe_analyzer_window_7d`.

## 6. E2E, docs, changelog

- [x] `integration/analyzer-grids.cjs`, `integration/shared-analyzer-data.cjs`:
      the recipe is a registered host.
- [x] `docs/shared-analyzer-filters.md`: T08 done.
- [x] `ultros-changelog/changes/2026-09-09-recipe-analyzer-window.json`.
- [x] `./check_ci.sh`.
