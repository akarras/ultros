# Item Explorer on MarketGrid — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Item Explorer's paginated `DataTableGrid` with `MarketGrid`, keeping every URL contract, the hydration gate and column availability, and requesting sale statistics only on demand.

**Architecture:** `ItemList` builds `ExplorerRow` values in one memo (native sort + `hq-only` only), hands them to `MarketGrid` with native `GridMetric`s that report `Pending` before hydration, and registers legacy filter keys as `FilterAlias`es. A new `use_market_data_on_demand` constructor in `analyzer_kit::market` wants no window at mount.

**Tech Stack:** Rust / Leptos 0.8 (SSR + hydrate), `analyzer_kit` (`MarketGrid`, `FilterRegistry`, `stat_columns`), leptos-i18n (7 locales), Puppeteer e2e in `integration/`.

Spec: `docs/superpowers/specs/2026-09-09-item-explorer-market-grid-design.md`.

## Global Constraints

- Run `./check_ci.sh` (fmt + clippy `-D warnings` + tests) before every commit; on Windows export `PATH=/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH` and `OPENSSL_RUST_USE_NASM=0` first. Log to a file inside the worktree, never `/tmp/ci.log`.
- No user-facing string literals in `view!`; every new key goes into all seven locale files (`en fr de ja cn ko tc`) with a real translation.
- `?sort=` tokens `ilvl lv price hq vendor world name key`, `?cols=` ids `ilvl lv hq vendor world`, and legacy filter keys `q min-ilvl max-ilvl min-lv max-price vendor-only hq-only listed` must keep working.
- Server render and first client render must produce the same rows in the same order (never read the listings resource before the `hydrated` effect).

---

### Task 1: `use_market_data_on_demand`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/market.rs:114-138`
- Test: same file, `mod tests`

**Interfaces:**
- Produces: `pub fn use_market_data_on_demand(scope: Signal<String>) -> MarketData` — identical to `use_market_data` except `wanted[*]` all start `false`.

- [ ] **Step 1: Failing test** — in `market.rs` tests:

```rust
#[test]
fn on_demand_market_data_wants_no_window_until_asked() {
    let _ = any_spawner::Executor::init_futures_executor();
    let owner = Owner::new();
    owner.with(|| {
        let scope = RwSignal::new("Gilgamesh".to_owned());
        let lazy = use_market_data_on_demand(scope.into());
        assert!(lazy.wanted.iter().all(|w| !w.get_untracked()));
        let eager = use_market_data(scope.into());
        assert!(eager.wanted[Window::D7.index()].get_untracked());
        lazy.want(Window::D30);
        assert!(lazy.wanted[Window::D30.index()].get_untracked());
        assert!(!lazy.wanted[Window::D7.index()].get_untracked());
    });
}
```

- [ ] **Step 2:** `cargo test -p ultros-app on_demand_market_data` → fails (function missing).
- [ ] **Step 3:** Refactor `use_market_data` into `fn market_data(scope, eager_7d: bool)`; `use_market_data` passes `true`, new `use_market_data_on_demand` passes `false`. Doc comment: the grid's needs effect wants windows from visible columns, `?cols=`, `?gf=` aliases and `?sort=grid:*`, so a page with no default market column makes no request.
- [ ] **Step 4:** test passes. Commit `feat(analyzer-kit): market data on demand`.

### Task 2: Explorer row model, subject and aliases (pure code + tests)

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_explorer_filters.rs` (add row model; shrink `ExplorerFilters`)
- Test: same file

**Interfaces (produced):**
```rust
pub struct ExplorerRow { pub item_id: i32, pub item: &'static Item, pub nq: Option<i32>, pub hq: Option<i32>,
    pub cheapest: Option<CheapestListing>, pub vendor: Option<u32>, pub prices_loaded: bool }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct CheapestListing { pub price: i32, pub hq: bool, pub world_id: i32 }
impl ExplorerRow {
    pub fn build(item_id: i32, item: &'static Item, prices: Option<&CheapestListingsMap>, vendor: Option<u32>) -> Self;
    pub fn market_subject(&self) -> MarketSubject;      // cheapest quality; NQ/world 0/None when absent
    pub fn price_value(&self, quality_hq: bool) -> GridValue; // Pending until loaded, Missing when none
    pub fn listing_value(&self) -> GridValue;            // cheapest either quality
}
pub const FILTER_NAME/MIN_ILVL/MAX_ILVL/MIN_LV/MAX_PRICE/FILTER_VENDOR/FILTER_HQ/FILTER_LISTED: &str (moved here)
pub fn explorer_filter_aliases() -> Vec<FilterAlias>;
pub struct ExplorerFilters { pub hq_only: bool }  // matches(&Item) -> bool
```
Column ids for metrics: `"item" "ilvl" "lv" "price" "hq" "vendor" "world" "key" "market-listing"`.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn subject_is_the_cheapest_listed_quality_or_nq_world_zero() {
    let item = first(GLADIATORS_ARMS);
    let map = |nq: Option<i32>, hq: Option<i32>| { /* build CheapestListingsMap with world 7 (nq) / 9 (hq) */ };
    let both = ExplorerRow::build(item.key_id.0, item, Some(&map(Some(500), Some(400))), None);
    assert_eq!(both.cheapest, Some(CheapestListing { price: 400, hq: true, world_id: 9 }));
    let s = both.market_subject();
    assert!((s.hq, s.world_id, s.listing_price) == (true, 9, Some(400)));
    let none = ExplorerRow::build(item.key_id.0, item, Some(&map(None, None)), None);
    assert_eq!(none.market_subject().world_id, 0);
    assert!(!none.market_subject().hq && none.market_subject().listing_price.is_none());
    let unloaded = ExplorerRow::build(item.key_id.0, item, None, None);
    assert_eq!(unloaded.price_value(false), GridValue::Pending);
    assert_eq!(none.price_value(false), GridValue::Missing);
    assert_eq!(both.listing_value(), GridValue::Number(400.0));
}

#[test]
fn legacy_filter_keys_resolve_to_inclusive_shared_bounds() {
    let aliases = explorer_filter_aliases();
    let mut q = ParamsMap::new();
    q.insert("min-ilvl", "600".into()); q.insert("max-ilvl", "700".into());
    q.insert("q", "sword".into()); q.insert("max-price", "1000".into());
    q.insert("vendor-only", "true".into()); q.insert("listed", "true".into());
    let f = resolve_filters(&q, &aliases);
    assert_eq!(f["ilvl"].op, FilterOp::Between); assert_eq!(f["ilvl"].value, "600,700");
    assert_eq!(f["item"].op, FilterOp::Contains);
    assert_eq!(f["market-listing"].op, FilterOp::Present); // listed wins? -> see Step 3: `max-price` and `listed` both target market-listing; Lte on a Missing row already excludes it, so `listed` maps to Present and max-price to Lte; when both are set, aliases combine only Gte/Lte so Present is dropped — assert Lte survives and note Lte already implies listed.
    let mut off = ParamsMap::new(); off.insert("vendor-only", "false".into());
    assert!(resolve_filters(&off, &aliases).is_empty());
}
```

- [ ] **Step 2:** run → fails.
- [ ] **Step 3:** Implement. Note for `max-price` + `listed` on the same column: `resolve_filters` keeps the first alias and only merges Gte/Lte, so order the aliases `max-price` before `listed`; an `Lte` bound already excludes unlisted rows, matching the legacy semantics. Boolean aliases use `convert: |raw| (raw == "true").then(|| "true".into())`.
- [ ] **Step 4:** tests pass. Commit `feat(explorer): row model, market subject and filter aliases`.

### Task 3: Rewrite `ItemList` on `MarketGrid`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_explorer.rs:507-1931` (delete pagination, chips, `use_explorer_filters`, `EXPLORER_COLUMNS`/`TrackWidths`; keep sort enum, availability, hydrated gate, ControlBar)
- Modify: `ultros-frontend/ultros-i18n/locales/*.json` (remove nothing; add `item_explorer_col_key` "Added" reuse `item_explorer_added` — no new keys expected)
- Modify: `ultros-frontend/ultros-app/Cargo.toml` only if `paginate` becomes unused across the crate (check with grep).

- [ ] **Step 1:** Build `rows: Memo<Vec<ExplorerRow>>`: from `items()`, price map gated by `hydrated`, filter `hq_only` (and nothing else), sort by `active_sort`/`direction` using the row fields (`cmp_none_last` for optional ones, stable).
- [ ] **Step 2:** `grid_columns: Signal<Vec<GridColumn>>`: `item` (330, required) + optional native columns present only when `availability.has(id)` (world only when multi-world), each `.sorted(...)` from `active_sort`/`active_dir`; `price` required; `actions` required (60, `auto_fit=false`).
- [ ] **Step 3:** `native_metrics`: as in Task 2 interface; `world` text uses the `worlds` helper from the cheapest listing world; `key` number = item id.
- [ ] **Step 4:** `register_filters(explorer_filter_aliases(), Signal::derive(|| vec![toggle_control(FILTER_HQ, label)]))` in `ItemList` (same owner as ControlBar + grid). Summary count = `filters.row_count()`.
- [ ] **Step 5:** ControlBar: keep sort select + direction button (drop `page` reset; use `set_params` via `use_navigate` replace), `MarketWindowControl window=market.window` in actions, `columns = native options (with reasons) + market_picker_options(window)`, `visible_columns = native visible ∪ shared_cols_in(cols)`, `on_toggle_column` handles native vs shared via `toggle_shared_col(prev, "", id)`; `on_reset_columns` clears `?cols=`.
- [ ] **Step 6:** `MarketGrid id="item-explorer-grid" label=t_string!(item_explorer_title_main) market=use_market_data_on_demand(scope_name) show_saved_views=false row_height=40 each=rows columns=grid_columns key=|r| (r.item_id) metrics subject=|r| r.market_subject() header=SortHeader per sortable id view=... measure=...`. Cells: item (icon+tooltip+link+name), ilvl, lv, price/hq via `<CheapestPrice>`, vendor `<Gil>`, world `<WorldName>` (gated by `hydrated` via row fields), actions (AddToList + Clipboard).
- [ ] **Step 7:** Delete pagination markup, `paginate` import, `QueryButton` import if unused, `RESET_ON_SORT`, chips. Update tests module: drop `paginate_oob_offset…`, `every_addable_filter_is_recognized`, `header_classes_match_their_tracks`, `sorts_and_filters_reference_real_columns` (replace with a test that every `ItemSortOption::column()` id is a grid column id).
- [ ] **Step 8:** `cargo check -p ultros-app` server+hydrate features; fix. Run explorer tests. Commit `feat(explorer): render the catalog on the shared market grid without pagination`.

### Task 4: Changelog, docs, e2e probe

**Files:**
- Create: `ultros-changelog/changes/2026-09-09-item-explorer-market-grid.json`
- Create: `integration/item-explorer-grid.cjs`; register in `integration/package.json` scripts and `integration/README.md`.

- [ ] **Step 1:** Changelog JSON (`category: "improvements"`, `importance: "medium"`, title "Item Explorer joins the shared market grid", blurb: no pages, sort/filter every column, optional sale-history columns per window, old links still work).
- [ ] **Step 2:** Probe (pattern from `integration/market-window.cjs`): open `/items/category/10`, wait `ultros:hydrated` + `.virtual-grid`, assert zero `sale_stats` requests; open `?cols=market-sale-median` → exactly one `sale_stats/<scope>?window=7`; `?page=3&per_page=25` → same row count as bare; `?min-ilvl=600&max-ilvl=700` → chip `[data-registered-filter]` present and count smaller; mobile 393px → `document.documentElement.scrollWidth <= innerWidth`.
- [ ] **Step 3:** Run the probe against a local server if one can be started (see AGENTS.md); otherwise note in PR.
- [ ] **Step 4:** Commit `test(explorer): grid regression probe and changelog`.

### Task 5: Gate and PR

- [ ] `./check_ci.sh > ci.log 2>&1; echo REAL_EXIT=$?` (foreground, 600000 ms). Fix anything red.
- [ ] Push branch, open PR with `Closes #1346`, list assumptions (Search Console check owed, first-viewport row count), attribution footer.
