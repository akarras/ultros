# Flip Finder Sale Metrics + Sale-Stat Window Matrix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> At execution time, copy this file to `docs/superpowers/plans/2026-09-07-flip-finder-sale-metrics.md` in the repo (plan mode could only write here) and commit it with Task 1.

**Goal:** Make the shared sale-history columns discoverable in the Flip Finder's Columns picker, extend the shared `market-*` column set to all four server windows (1d / 7d / 30d / 90d) for every statistic, and add a "Gil traded" statistic end to end.

**Architecture:** The Flip Finder already renders the shared `MarketGrid` (`analyzer_kit/market.rs`), which appends every `market-*` column hidden-by-default; the picker gap is a wiring gap in `routes/analyzer.rs`, not a data gap. The window expansion replaces the flat `Median7 / Units30 …` enum variants with `MarketMetric::Stat(StatKind, Window)` over a static table of literal column ids in a new `analyzer_kit/stat_columns.rs`, and generalizes `MarketData` from two fixed slots to one per window with the same on-demand gating the 30d slot has today. Gil volume is already summed in ClickHouse for VWAP; it is exposed as one more row/API field and one more `StatKind`.

**Tech Stack:** Rust workspace: Leptos 0.8 (`ultros-frontend/ultros-app`), axum (`ultros/`), ClickHouse (`ultros-clickhouse/`), `ultros-api-types` wire types, `leptos-i18n` locales in `ultros-frontend/ultros-app/locales/*.json`, Puppeteer e2e in `integration/`.

## Context

The user reported that every analyzer has "Sale median (7d)" and "Sale average (7d)" except the Flip Finder, and asked for a port plan plus the LOE for more windows and "total sold" per window.

Verified on prod (`/pkg/eb02c97/`, current `main`) on 2026-09-07:

- `/flip-finder/Gilgamesh?cols=…,market-sale-median-7,market-sale-avg-7` renders both columns with real values. The Flip Finder is one of six `MarketGrid` consumers (`routes/analyzer.rs:2821`), so the shared metrics, i18n keys, `?sort=grid:` sort and column-menu filters all already work there.
- The Flip Finder is the only analyzer with a **toolbar Columns picker**, and it lists only the 11 native ids in `ALL_OPTIONAL_COLS` (`routes/analyzer.rs:1657`). The market columns are reachable only via header "⋮ → Insert column after… → search". That is the whole "missing" feeling.
- Windows: the server (`ultros/src/web/api/sale_stats.rs:35`) accepts `window=1|7|30|90`, ClickHouse materializes all four (`ultros-clickhouse/src/rollups.rs:542-652`), the response cache keys on window. Only the frontend bakes `7`/`30` into the enum, the column ids, the two `MarketData` slots and the labels (`analyzer_kit/market.rs:46-100, 256-356`).
- "Total sold": Units sold / Sales counts already exist for 7d and 30d; the user actually wants **gil volume**, which `bulk_sale_stats` sums (`ultros-clickhouse/src/queries.rs:1068`) but does not return.

Decisions made with the user: (1) list the shared columns in the picker (no default-on, no pre-declared position, no native `SortMode`); (2) extra columns per window, not a page-level window selector; (3) full statistic set for every window; (4) picker lists every window, grouped by window heading.

## Global Constraints

- Run `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log` before every commit (fmt + clippy `-D warnings` + tests). Never `#[allow]` a lint to pass.
- Every user-facing string goes through `leptos-i18n`; every new key lands in all seven locale files (`en, fr, de, ja, cn, ko, tc`) with real translations, `snake_case`, `market_` prefix.
- Existing column ids are a bookmark / saved-view contract and must stay byte-identical: `market-sale-min-7`, `market-sale-median-7`, `market-sale-avg-7`, `market-sales-per-day-7`, `market-cadence-7`, `market-units-7`, `market-sales-7`, `market-vwap-7`, `market-units-30`, `market-sales-30`, `market-vwap-30`.
- Existing locale keys `market_sale_min_7`, `market_sale_median_7`, `market_sale_avg_7`, `market_sales_per_day_7`, `market_sales_30_cleaned` stay (used by `MarketPriceControls` and the Flip Finder's native labels).
- `ItemSaleStats` wire additions are `#[serde(default)]` (old servers / cached bodies must still deserialize; test `old_wire_shape_still_deserializes`).
- Never build a `[RwSignal::new(None); 4]` array literal: `RwSignal` is `Copy`, so that clones ONE signal four times. Use `std::array::from_fn`.
- A bare `RwSignal::new` / `Memo::new` in a unit test panics without an `Owner` (memory: `reference_leptos_test_owner_arena`); wrap reactive tests in `Owner::new().with(|| …)` like `components/control_bar.rs:500-514`.
- Recipe Analyzer keeps its own `rev-sale-*` / `cost-sale-*` columns and its own 30d store (`routes/recipe_analyzer.rs:615`); it is out of scope and must not be touched.
- Local flip-finder enrichment fetches may never fire (memory: `reference_ultros_local_flip_enrichment_never_fires`); final verification of network behaviour is on prod after deploy.

## LOE summary (what the user asked for)

| Piece | Where | Size |
|---|---|---|
| Picker listing (Task 4) | `routes/analyzer.rs` + a helper in the new module | ~80 lines, half a day incl. tests |
| Window matrix (Tasks 2–3) | new `analyzer_kit/stat_columns.rs`, `analyzer_kit/market.rs` refactor, 11 locale keys × 7 files | ~400 lines, 1–1.5 days; 36 columns from one table |
| Gil volume (Task 1) | `ultros-clickhouse/src/queries.rs`, `ultros-api-types/src/sale_stats.rs`, `ultros/src/web/api/sale_stats.rs`, smoke test | ~30 lines, plus a prod curl after deploy |
| E2E + changelog (Task 5) | `integration/shared-analyzer-data.cjs`, `ultros-changelog/changes/` | small |

Code reuse across analyzers is total for the column layer: all six `MarketGrid` consumers get the new windows and gil columns with no per-page change. Only the Flip Finder needs page code (the picker). Server and cache need nothing for windows.

## File Structure

- **Create** `ultros-frontend/ultros-app/src/analyzer_kit/stat_columns.rs` — `Window`, `StatKind`, `StatColumn`, the `STAT_COLUMNS` id table, `stat_label`, `window_wanted`, `market_picker_options`. One responsibility: "which sale-stat columns exist, what are they called, when is a window needed".
- **Modify** `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs` — register the module.
- **Modify** `ultros-frontend/ultros-app/src/analyzer_kit/needed.rs:27-38` — make `is_supported_window` `pub(crate)` so the table asserts against the server's set at compile time.
- **Modify** `ultros-frontend/ultros-app/src/analyzer_kit/market.rs` — `MarketData` per-window slots, `MarketMetric::Stat`, `market_metrics()` / `metric_by_id()`, gating effect, `stats_value` / `display_value` / `market_value` arms, `sizing_version`.
- **Modify** `ultros-frontend/ultros-app/src/routes/analyzer.rs:1657-1679` — picker options, checked-state memo, toggle for shared ids; tests near `:3817`.
- **Modify** `ultros-clickhouse/src/queries.rs:999-1081`, `ultros-clickhouse/tests/sale_stats_smoke.rs:117-122`, `ultros-api-types/src/sale_stats.rs`, `ultros/src/web/api/sale_stats.rs:110-128` — gil volume.
- **Modify** all seven `ultros-frontend/ultros-app/locales/*.json` — 11 new keys.
- **Modify** `integration/shared-analyzer-data.cjs:200` — two more required shared ids.
- **Create** `ultros-changelog/changes/2026-09-07-sale-history-windows.json`.

---

### Task 1: Gil volume through ClickHouse, the API type and the handler

**Files:**
- Modify: `ultros-clickhouse/src/queries.rs:999-1081`
- Modify: `ultros-clickhouse/tests/sale_stats_smoke.rs:117-122`
- Modify: `ultros-api-types/src/sale_stats.rs`
- Modify: `ultros/src/web/api/sale_stats.rs:110-128`

**Interfaces:**
- Produces: `BulkSaleStatsRow.gil_volume: u64` (last field, matches SELECT order — the `clickhouse` `Row` derive is positional), `ItemSaleStats.gil_volume: u64` (serde-defaulted), populated by the handler.

- [ ] **Step 1: Extend the API type test to expect a defaulted `gil_volume`**

In `ultros-api-types/src/sale_stats.rs`, add the field after `vwap`:

```rust
    /// Total gil traded in the window (sum of price × quantity). 0 = unknown.
    #[serde(default)]
    pub gil_volume: u64,
```

and extend `old_wire_shape_still_deserializes`:

```rust
        assert_eq!(row.gil_volume, 0);
```

- [ ] **Step 2: Run the type test**

Run: `cargo test -p ultros-api-types old_wire_shape_still_deserializes`
Expected: PASS (serde default). Then `cargo check --workspace` — every non-test `ItemSaleStats { … }` literal without `..Default::default()` now fails. The only production one is the handler (fixed in Step 4). Test-only literals that fail (`analyzer_kit/signals.rs:163`, `price_basis.rs:105`, `routes/recipe_analyzer.rs:6042`, and any others the compiler names) get `gil_volume: 0,` added; do not change their behaviour.

- [ ] **Step 3: Return the sum from ClickHouse**

In `ultros-clickhouse/src/queries.rs`, add to `BulkSaleStatsRow` (last, after `vwap`):

```rust
    /// Total gil traded in the window: `sum(price_per_item * quantity)`.
    pub gil_volume: u64,
```

and in the **outer** SELECT of `bulk_sale_stats`, after the `vwap` line:

```sql
            toInt32(round(gil_volume_sum / greatest(units_sold, 1))) AS vwap,
            gil_volume_sum AS gil_volume
```

The outer select only forwards the inner alias, so this does not hit the same-scope alias trap documented on the function (`ILLEGAL_AGGREGATION`).

- [ ] **Step 4: Map it in the handler**

`ultros/src/web/api/sale_stats.rs`, inside the `ItemSaleStats { … }` mapping after `vwap: r.vwap,`:

```rust
            gil_volume: r.gil_volume,
```

- [ ] **Step 5: Assert it in the smoke test**

`ultros-clickhouse/tests/sale_stats_smoke.rs`, after `assert_eq!(row.vwap, 175);`:

```rust
    // 100 × 1 + 200 × 3.
    assert_eq!(row.gil_volume, 700);
```

- [ ] **Step 6: Run what can run**

Run: `cargo check --workspace` then `cargo test -p ultros-api-types -p ultros-clickhouse`
Expected: compile clean; the smoke test prints `skipped: set ULTROS_CH_INTEGRATION=1 to run` unless a throwaway ClickHouse is up (the file header has the `docker run` recipe — run it if Docker is available, `gil_volume == 700` must hold).

- [ ] **Step 7: CI check and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add ultros-clickhouse/src/queries.rs ultros-clickhouse/tests/sale_stats_smoke.rs ultros-api-types/src/sale_stats.rs ultros/src/web/api/sale_stats.rs
git commit -m "feat(sale_stats): expose gil traded per window"
```

---

### Task 2: The window × statistic table (`stat_columns.rs`) and its locale keys

**Files:**
- Create: `ultros-frontend/ultros-app/src/analyzer_kit/stat_columns.rs`
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs`
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/needed.rs:29` (`const fn is_supported_window` → `pub(crate) const fn`)
- Modify: `ultros-frontend/ultros-app/locales/{en,de,fr,ja,cn,ko,tc}.json`

**Interfaces:**
- Produces:
  - `pub enum Window { D1, D7, D30, D90 }` with `pub const ALL: [Window; 4]`, `pub const fn days(self) -> u16`, `pub const fn index(self) -> usize`.
  - `pub enum StatKind { Min, Median, Average, SalesPerDay, Cadence, Units, Sales, Vwap, GilVolume }`.
  - `pub struct StatColumn { pub kind: StatKind, pub window: Window, pub id: &'static str }` and `pub const STAT_COLUMNS: [StatColumn; 36]` (window-major, kind order as in the enum).
  - `pub fn stat_column(kind: StatKind, window: Window) -> &'static StatColumn`.
  - `pub fn stat_label(kind: StatKind, window: Window) -> String`.
  - `pub fn window_wanted(needs: &HashSet<String>, window: Window) -> bool`.
  - `pub fn market_picker_options() -> Vec<ColumnOption>`.

- [ ] **Step 1: Add the locale keys (all seven files)**

Insert directly after the `"market_vwap_30"` line in each file (the `market_*` block, en.json:49). Values:

| key | en | de | fr | ja | cn | tc | ko |
|---|---|---|---|---|---|---|---|
| `market_stat_window` | `{{stat}} ({{days}}d)` | `{{stat}} ({{days}} T.)` | `{{stat}} ({{days}} j)` | `{{stat}}（{{days}}日）` | `{{stat}}（{{days}}天）` | `{{stat}}（{{days}}天）` | `{{stat}} ({{days}}일)` |
| `market_stat_sale_min` | Sale minimum | Minimaler Verkaufspreis | Prix de vente minimum | 最低販売価格 | 最低成交价 | 最低成交價 | 최저 판매가 |
| `market_stat_sale_median` | Sale median | Medianverkaufspreis | Prix de vente médian | 販売価格中央値 | 成交价中位数 | 成交價中位數 | 판매가 중앙값 |
| `market_stat_sale_avg` | Sale average | Durchschnittlicher Verkaufspreis | Prix de vente moyen | 平均販売価格 | 平均成交价 | 平均成交價 | 평균 판매가 |
| `market_stat_sales_per_day` | Sales/day | Verkäufe/Tag | Ventes/jour | 販売件数/日 | 日均成交笔数 | 每日成交筆數 | 일일 판매 건수 |
| `market_stat_cadence` | Hours/sale | Stunden/Verkauf | Heures/vente | 時間/販売1件 | 每笔成交小时数 | 每筆成交小時數 | 판매 1건당 시간 |
| `market_stat_units` | Units sold | Verkaufte Einheiten | Unités vendues | 販売個数 | 售出数量 | 售出數量 | 판매 수량 |
| `market_stat_sales` | Sales | Verkäufe | Ventes | 販売件数 | 成交笔数 | 成交筆數 | 판매 건수 |
| `market_stat_vwap` | VWAP | VWAP | VWAP | VWAP | 成交量加权均价 | 成交量加權均價 | VWAP |
| `market_stat_gil` | Gil traded | Gehandelte Gil | Gils échangés | 取引ギル総額 | 成交金额 | 成交金額 | 거래 길 |
| `market_picker_group_history` | Sale history | Verkaufshistorie | Historique des ventes | 販売履歴 | 成交历史 | 成交歷史 | 판매 이력 |

The stat names are the existing `market_*_7` values with their window suffix removed, so `stat_label(Median, D7)` reproduces today's header text exactly in every locale.

- [ ] **Step 2: Write the failing tests**

Create `ultros-frontend/ultros-app/src/analyzer_kit/stat_columns.rs` with only the tests module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn legacy_ids_are_preserved_and_all_ids_are_unique() {
        for id in [
            "market-sale-min-7", "market-sale-median-7", "market-sale-avg-7",
            "market-sales-per-day-7", "market-cadence-7", "market-units-7",
            "market-sales-7", "market-vwap-7", "market-units-30", "market-sales-30",
            "market-vwap-30",
        ] {
            assert!(STAT_COLUMNS.iter().any(|c| c.id == id), "lost {id}");
        }
        let ids: HashSet<_> = STAT_COLUMNS.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), STAT_COLUMNS.len());
        assert_eq!(STAT_COLUMNS.len(), 9 * Window::ALL.len());
    }

    #[test]
    fn every_id_ends_with_its_window() {
        for c in &STAT_COLUMNS {
            assert!(c.id.ends_with(&format!("-{}", c.window.days())), "{}", c.id);
            assert!(std::ptr::eq(stat_column(c.kind, c.window), c));
        }
    }

    #[test]
    fn a_window_is_wanted_only_when_one_of_its_columns_is() {
        let needs: HashSet<String> = ["market-sale-median-30", "roi"].map(str::to_owned).into();
        assert!(window_wanted(&needs, Window::D30));
        assert!(!window_wanted(&needs, Window::D90));
        assert!(!window_wanted(&needs, Window::D1));
    }

    #[test]
    fn labels_and_picker_match_the_legacy_seven_day_text() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = leptos::prelude::Owner::new();
        owner.with(|| {
            leptos::prelude::provide_context(leptos_i18n::init_i18n_context::<crate::i18n::Locale>());
            assert_eq!(stat_label(StatKind::Median, Window::D7), "Sale median (7d)");
            assert_eq!(stat_label(StatKind::GilVolume, Window::D90), "Gil traded (90d)");
            let options = market_picker_options();
            assert_eq!(options.len(), STAT_COLUMNS.len());
            let median = options.iter().find(|o| o.id == "market-sale-median-7").unwrap();
            assert_eq!(median.label, "Sale median (7d)");
            assert_eq!(
                median.group.as_ref().map(|g| g.label.as_str()),
                Some("Sale history (7d)")
            );
            assert_eq!(options[0].id, "market-sale-min-1");
        });
    }
}
```

Use the exact `init_i18n_context` import path `components/control_bar.rs:504` uses (`crate::i18n::*` re-exports it as `init_i18n_context`); mirror that file.

- [ ] **Step 3: Register the module and run the tests to see them fail**

Add `pub mod stat_columns;` to `analyzer_kit/mod.rs` (alphabetical, after `signals`).

Run: `cargo test -p ultros-app stat_columns`
Expected: FAIL to compile — `STAT_COLUMNS`, `Window`, … not defined.

- [ ] **Step 4: Implement the module**

Above the tests in `stat_columns.rs`:

```rust
//! The window × statistic matrix behind the shared `market-*` sale-history
//! columns: the windows the server serves, the statistics each carries, the
//! literal column ids (a bookmark and saved-view contract), the labels, and
//! the Columns-picker options built from them. Every analyzer that renders
//! `MarketGrid` gets all of these; the Flip Finder also lists them in its
//! toolbar picker.

use std::collections::HashSet;

use crate::components::control_bar::{ColumnOption, PickerHeading};
use crate::i18n::*;

use super::needed::is_supported_window;

/// A trailing sale-history window `/api/v1/sale_stats` serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Window {
    D1,
    D7,
    D30,
    D90,
}

impl Window {
    pub const ALL: [Window; 4] = [Window::D1, Window::D7, Window::D30, Window::D90];

    pub const fn days(self) -> u16 {
        match self {
            Window::D1 => 1,
            Window::D7 => 7,
            Window::D30 => 30,
            Window::D90 => 90,
        }
    }

    /// Position in [`Window::ALL`]; indexes `MarketData`'s per-window slots.
    pub const fn index(self) -> usize {
        match self {
            Window::D1 => 0,
            Window::D7 => 1,
            Window::D30 => 2,
            Window::D90 => 3,
        }
    }
}

const _: () = {
    let mut i = 0;
    while i < Window::ALL.len() {
        assert!(
            is_supported_window(Window::ALL[i].days()),
            "a Window the server does not serve"
        );
        i += 1;
    }
};

/// One statistic read from an `ItemSaleStats` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatKind {
    Min,
    Median,
    Average,
    SalesPerDay,
    Cadence,
    Units,
    Sales,
    Vwap,
    GilVolume,
}

#[derive(Debug)]
pub struct StatColumn {
    pub kind: StatKind,
    pub window: Window,
    pub id: &'static str,
}

/// Ids are literals so `grep market-sale-median-7` finds the contract.
macro_rules! window_columns {
    ($($days:literal => $w:ident),* $(,)?) => {
        [$(
            StatColumn { kind: StatKind::Min,         window: Window::$w, id: concat!("market-sale-min-", $days) },
            StatColumn { kind: StatKind::Median,      window: Window::$w, id: concat!("market-sale-median-", $days) },
            StatColumn { kind: StatKind::Average,     window: Window::$w, id: concat!("market-sale-avg-", $days) },
            StatColumn { kind: StatKind::SalesPerDay, window: Window::$w, id: concat!("market-sales-per-day-", $days) },
            StatColumn { kind: StatKind::Cadence,     window: Window::$w, id: concat!("market-cadence-", $days) },
            StatColumn { kind: StatKind::Units,       window: Window::$w, id: concat!("market-units-", $days) },
            StatColumn { kind: StatKind::Sales,       window: Window::$w, id: concat!("market-sales-", $days) },
            StatColumn { kind: StatKind::Vwap,        window: Window::$w, id: concat!("market-vwap-", $days) },
            StatColumn { kind: StatKind::GilVolume,   window: Window::$w, id: concat!("market-gil-", $days) },
        )*]
    };
}

/// Window-major, kind order as declared: this is also the picker order.
pub const STAT_COLUMNS: [StatColumn; 36] =
    window_columns!(1 => D1, 7 => D7, 30 => D30, 90 => D90);

pub fn stat_column(kind: StatKind, window: Window) -> &'static StatColumn {
    STAT_COLUMNS
        .iter()
        .find(|c| c.kind == kind && c.window == window)
        .expect("every (kind, window) pair is in STAT_COLUMNS")
}

fn stat_name(kind: StatKind) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match kind {
        StatKind::Min => t_string!(i18n, market_stat_sale_min),
        StatKind::Median => t_string!(i18n, market_stat_sale_median),
        StatKind::Average => t_string!(i18n, market_stat_sale_avg),
        StatKind::SalesPerDay => t_string!(i18n, market_stat_sales_per_day),
        StatKind::Cadence => t_string!(i18n, market_stat_cadence),
        StatKind::Units => t_string!(i18n, market_stat_units),
        StatKind::Sales => t_string!(i18n, market_stat_sales),
        StatKind::Vwap => t_string!(i18n, market_stat_vwap),
        StatKind::GilVolume => t_string!(i18n, market_stat_gil),
    }
    .to_string()
}

/// "`{name}` (7d)" in the locale's own suffix form.
fn with_window(name: String, window: Window) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    t_string!(
        i18n,
        market_stat_window,
        stat = name,
        days = window.days().to_string()
    )
    .to_string()
}

pub fn stat_label(kind: StatKind, window: Window) -> String {
    with_window(stat_name(kind), window)
}

/// Whether any column of `window` is in the grid's wanted set (`?cols=`,
/// `?gf=` filters, the `?sort=grid:` target, visible defs).
pub fn window_wanted(needs: &HashSet<String>, window: Window) -> bool {
    STAT_COLUMNS
        .iter()
        .any(|c| c.window == window && needs.contains(c.id))
}

/// Every stat column as a toolbar-picker option, grouped under one
/// "Sale history (Nd)" heading per window.
pub fn market_picker_options() -> Vec<ColumnOption> {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let history = t_string!(i18n, market_picker_group_history).to_string();
    STAT_COLUMNS
        .iter()
        .map(|c| ColumnOption {
            id: c.id,
            label: stat_label(c.kind, c.window),
            group: Some(PickerHeading {
                label: with_window(history.clone(), c.window),
                title: None,
            }),
            disabled: false,
            hint: None,
        })
        .collect()
}
```

In `needed.rs:29` change `const fn is_supported_window` to `pub(crate) const fn is_supported_window`.

If `t_string!` with interpolation refuses a `String` for `days`, pass `days = window.days()` (the `count = n` call at `routes/analyzer.rs:2335` shows integers are accepted); adjust the JSON only if the macro demands a typed variable.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ultros-app stat_columns`
Expected: 4 PASS. (`cargo test -p ultros-app` also compiles the `#[cfg(test)]` i18n owner path; missing locale keys in any file only WARN at build — grep each of the seven files for `market_picker_group_history` to be sure all landed.)

- [ ] **Step 6: CI check and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add ultros-frontend/ultros-app/src/analyzer_kit/stat_columns.rs ultros-frontend/ultros-app/src/analyzer_kit/mod.rs ultros-frontend/ultros-app/src/analyzer_kit/needed.rs ultros-frontend/ultros-app/locales/
git commit -m "feat(analyzer-kit): window × statistic table for shared sale-history columns"
```

---

### Task 3: Drive `MarketGrid` from the table (per-window slots, `MarketMetric::Stat`)

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/market.rs:42-138` (handle + fetch), `:256-424` (enum, ids, labels, values), `:490-579` (`market_value`, `display_value`), `:635-642` (`sizing_version`), `:652-666` (`all_columns`), `:685-695` (want gate), `:755-771` (metric registration), `:787-825` (header/view/measure lookups), tests `:869-951`.

**Interfaces:**
- Consumes: `Window`, `StatKind`, `STAT_COLUMNS`, `stat_column`, `stat_label`, `window_wanted` from Task 2; `ItemSaleStats.gil_volume` from Task 1.
- Produces: `MarketData::stats(self, Window) -> Option<Arc<StatsIndex>>`, `MarketData::stats_failed(self, Window) -> bool`, `MarketData::stats7()` / `stats30()` kept as thin wrappers (six route files call `stats7()`; nothing outside this file may break).

- [ ] **Step 1: Update the tests first**

In `market.rs` tests replace `shared_column_ids_are_unique` and `counts_and_velocity_describe_sales_separately_from_units` with:

```rust
    #[test]
    fn shared_column_ids_are_unique_and_include_every_window() {
        let ids: Vec<_> = market_metrics().map(|m| m.id()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len());
        for id in ["market-sale-median-7", "market-sale-median-30", "market-gil-1", "market-vwap-90", "market-trend-7"] {
            assert!(unique.contains(id), "{id}");
            assert_eq!(metric_by_id(id).map(|m| m.id()), Some(id));
        }
        assert_eq!(metric_by_id("roi"), None);
    }

    #[test]
    fn counts_and_velocity_describe_sales_separately_from_units() {
        let stats = Some(ItemSaleStats {
            num_sold: 14,
            units_sold: 140,
            sales_per_day: 2.0,
            gil_volume: 7_000,
            ..Default::default()
        });
        let stat = |kind| stats_value(MarketMetric::Stat(kind, Window::D7), stats);
        assert_eq!(stat(StatKind::Units), GridValue::Number(140.0));
        assert_eq!(stat(StatKind::Sales), GridValue::Number(14.0));
        assert_eq!(stat(StatKind::Cadence), GridValue::Number(12.0));
        assert_eq!(stat(StatKind::GilVolume), GridValue::Number(7_000.0));
        assert_eq!(stat(StatKind::Median), GridValue::Missing);
        assert_eq!(
            stats_value(MarketMetric::Stat(StatKind::GilVolume, Window::D7), Some(ItemSaleStats::default())),
            GridValue::Missing,
            "an old server's zero is unknown, not free"
        );
    }

    #[test]
    fn rates_format_with_two_decimals_and_gil_with_commas() {
        assert_eq!(display_value(MarketMetric::Stat(StatKind::SalesPerDay, Window::D30), GridValue::Number(2.5)), "2.50");
        assert_eq!(display_value(MarketMetric::Stat(StatKind::GilVolume, Window::D7), GridValue::Number(1234567.0)), "1,234,567");
    }
```

`MarketMetric` needs `PartialEq, Debug` derives for these asserts.

Run: `cargo test -p ultros-app analyzer_kit::market`
Expected: FAIL to compile (`market_metrics`, `metric_by_id`, `MarketMetric::Stat` missing).

- [ ] **Step 2: Replace the handle**

Replace lines 42-102 (`ScopedStats` through `use_market_data`) with:

```rust
type ScopedStats = Option<(String, Arc<StatsIndex>, bool)>;

/// A cheap reactive handle; the payloads are cloned only by Arc. One slot
/// per server window, indexed by `Window::index()`. The seven-day body is
/// always fetched; the others only once a column of theirs is wanted.
#[derive(Clone, Copy)]
pub struct MarketData {
    pub scope: Signal<String>,
    stats: [RwSignal<ScopedStats>; Window::ALL.len()],
    wanted: [RwSignal<bool>; Window::ALL.len()],
}

impl MarketData {
    pub fn stats(self, window: Window) -> Option<Arc<StatsIndex>> {
        let scope = self.scope.get();
        self.stats[window.index()].with(|v| {
            v.as_ref()
                .filter(|(name, _, _)| name == &scope)
                .map(|(_, stats, _)| stats.clone())
        })
    }

    pub fn stats_failed(self, window: Window) -> bool {
        let scope = self.scope.get();
        self.stats[window.index()]
            .with(|v| v.as_ref().is_some_and(|(name, _, failed)| name == &scope && *failed))
    }

    pub fn stats7(self) -> Option<Arc<StatsIndex>> {
        self.stats(Window::D7)
    }

    pub fn stats30(self) -> Option<Arc<StatsIndex>> {
        self.stats(Window::D30)
    }

    /// Ask for a window's body. Idempotent; never un-wants.
    fn want(self, window: Window) {
        let flag = self.wanted[window.index()];
        if !flag.get_untracked() {
            flag.set(true);
        }
    }

    /// Subscribe the caller to every slot without copying a payload.
    fn track_all(self) {
        for slot in self.stats {
            slot.with(|_| ());
        }
    }
}

/// Both SSR and the initial hydrated render use listing fallbacks. The
/// client fills the shared seven-day body after mounting; the other windows
/// never delay the first table. Failed requests settle to empty.
pub fn use_market_data(scope: Signal<String>) -> MarketData {
    // `RwSignal` is `Copy`: a `[RwSignal::new(None); 4]` literal would be one
    // signal four times over.
    let market = MarketData {
        scope,
        stats: std::array::from_fn(|_| RwSignal::new(None)),
        wanted: std::array::from_fn(|i| RwSignal::new(i == Window::D7.index())),
    };
    for window in Window::ALL {
        fetch_stats(
            scope,
            market.stats[window.index()],
            market.wanted[window.index()].into(),
            window.days(),
        );
    }
    market
}
```

`fetch_stats` (lines 104-138) is unchanged. Add `use super::stat_columns::{STAT_COLUMNS, StatKind, Window, stat_column, stat_label, window_wanted};` to the `use super::{…}` block.

- [ ] **Step 3: Replace the metric enum, ids, labels and value extraction**

Replace lines 256-424 (`enum MarketMetric` through `stats_value`) with:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarketMetric {
    Subject,
    Scope,
    Quality,
    World,
    Datacenter,
    Listing,
    /// One statistic of one window; ids and labels come from `STAT_COLUMNS`.
    Stat(StatKind, Window),
    LastSold,
    Confidence,
    TrendWorld,
    Trend7,
    Drift7,
}

impl MarketMetric {
    fn id(self) -> &'static str {
        match self {
            Self::Subject => "market-subject",
            Self::Scope => "market-scope",
            Self::Quality => "market-quality",
            Self::World => "market-world",
            Self::Datacenter => "market-datacenter",
            Self::Listing => "market-listing",
            Self::Stat(kind, window) => stat_column(kind, window).id,
            Self::LastSold => "market-last-sold",
            Self::Confidence => "market-confidence",
            Self::TrendWorld => "market-trend-world",
            Self::Trend7 => "market-trend-7",
            Self::Drift7 => "market-drift-7",
        }
    }

    fn text(self) -> bool {
        matches!(
            self,
            Self::Subject
                | Self::Scope
                | Self::Quality
                | Self::World
                | Self::Datacenter
                | Self::Confidence
                | Self::LastSold
                | Self::TrendWorld
        )
    }

    fn partial(self) -> bool {
        matches!(self, Self::Trend7 | Self::Drift7)
    }

    /// The bulk body this metric reads. Last-sold and confidence are
    /// seven-day facts, as before.
    fn window(self) -> Option<Window> {
        match self {
            Self::Stat(_, window) => Some(window),
            Self::LastSold | Self::Confidence => Some(Window::D7),
            _ => None,
        }
    }
}

const LEADING_METRICS: [MarketMetric; 6] = [
    MarketMetric::Subject,
    MarketMetric::Scope,
    MarketMetric::Quality,
    MarketMetric::World,
    MarketMetric::Datacenter,
    MarketMetric::Listing,
];

const TRAILING_METRICS: [MarketMetric; 5] = [
    MarketMetric::LastSold,
    MarketMetric::Confidence,
    MarketMetric::TrendWorld,
    MarketMetric::Trend7,
    MarketMetric::Drift7,
];

/// Every shared column in default (appended) order: identity, then the
/// window × statistic table, then the seven-day text and trend columns.
fn market_metrics() -> impl Iterator<Item = MarketMetric> {
    LEADING_METRICS
        .into_iter()
        .chain(STAT_COLUMNS.iter().map(|c| MarketMetric::Stat(c.kind, c.window)))
        .chain(TRAILING_METRICS)
}

fn metric_by_id(id: &str) -> Option<MarketMetric> {
    market_metrics().find(|m| m.id() == id)
}

fn metric_label(metric: MarketMetric) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match metric {
        MarketMetric::Subject => t_string!(i18n, market_subject),
        MarketMetric::Scope => t_string!(i18n, market_scope),
        MarketMetric::Quality => t_string!(i18n, market_quality),
        MarketMetric::World => t_string!(i18n, market_world),
        MarketMetric::Datacenter => t_string!(i18n, market_datacenter),
        MarketMetric::Listing => t_string!(i18n, market_listing),
        MarketMetric::Stat(kind, window) => return stat_label(kind, window),
        MarketMetric::LastSold => t_string!(i18n, market_last_sold),
        MarketMetric::Confidence => t_string!(i18n, market_confidence),
        MarketMetric::TrendWorld => t_string!(i18n, market_trend_world),
        MarketMetric::Trend7 => t_string!(i18n, market_trend_7),
        MarketMetric::Drift7 => t_string!(i18n, market_drift_7),
    }
    .to_string()
}

fn number(value: Option<f64>) -> GridValue {
    value
        .filter(|v| v.is_finite())
        .map_or(GridValue::Missing, GridValue::Number)
}

fn stats_value(metric: MarketMetric, stats: Option<ItemSaleStats>) -> GridValue {
    let Some(s) = stats else {
        return GridValue::Missing;
    };
    let positive = |v: i32| (v > 0).then_some(f64::from(v));
    match metric {
        MarketMetric::Confidence => match s.confidence {
            ConfidenceBand::Unknown => GridValue::Missing,
            band => GridValue::Text(format!("{band:?}")),
        },
        MarketMetric::LastSold => chrono::DateTime::from_timestamp(s.last_sold_unix, 0)
            .filter(|_| s.last_sold_unix > 0)
            .map_or(GridValue::Missing, |time| {
                GridValue::Text(time.format("%Y-%m-%d %H:%M UTC").to_string())
            }),
        MarketMetric::Stat(kind, _) => number(match kind {
            StatKind::Min => positive(s.min_price),
            StatKind::Median => positive(s.median_price),
            StatKind::Average => positive(s.avg_price),
            StatKind::SalesPerDay => Some(f64::from(s.sales_per_day)),
            StatKind::Cadence => {
                (s.sales_per_day > 0.0).then(|| 24.0 / f64::from(s.sales_per_day))
            }
            StatKind::Units => Some(s.units_sold as f64),
            StatKind::Sales => Some(s.num_sold as f64),
            StatKind::Vwap => positive(s.vwap),
            // Zero is an old server (serde default), not a free market.
            StatKind::GilVolume => (s.gil_volume > 0).then(|| s.gil_volume as f64),
        }),
        _ => GridValue::Missing,
    }
}
```

Delete `MARKET_METRICS` and the `thirty_days()` helper (both gone above).

- [ ] **Step 4: Rewrite the value / display arms**

In `market_value` (line 490) replace the final `_ => { … }` arm with:

```rust
        _ => {
            let Some(window) = metric.window() else {
                return GridValue::Missing;
            };
            if market.stats_failed(window) {
                return GridValue::Unavailable;
            }
            match market.stats(window) {
                None => GridValue::Pending,
                Some(stats) => {
                    let value =
                        stats_value(metric, stats.get(&(subject.item_id, subject.hq)).copied());
                    if matches!(metric, MarketMetric::Confidence)
                        && matches!(value, GridValue::Text(_))
                    {
                        GridValue::Text(display_value(metric, value))
                    } else {
                        value
                    }
                }
            }
        }
```

In `display_value` (line 550) change the two-decimal arm to:

```rust
        GridValue::Number(n)
            if matches!(
                metric,
                MarketMetric::Stat(StatKind::SalesPerDay | StatKind::Cadence, _)
            ) =>
        {
            format!("{n:.2}")
        }
```

- [ ] **Step 5: Rewire the component body**

- `sizing_version` (lines 635-642): replace the two `market.stats_7.with(|_| ()); market.stats_30.with(|_| ());` lines with `market.track_all();`.
- `all_columns` (lines 652-666): `for metric in MARKET_METRICS {` → `for metric in market_metrics() {`.
- Want gate (lines 685-695): replace the whole `Effect::new(…)` with

```rust
    Effect::new(move |_| {
        needs.with(|n| {
            for window in Window::ALL {
                if window != Window::D7 && window_wanted(n, window) {
                    market.want(window);
                }
            }
        });
    });
```

- Metric registration (lines 755-771): `for metric in MARKET_METRICS {` → `for metric in market_metrics() {`.
- Header / view / measure closures (lines 787, 792, 813): replace each `MARKET_METRICS.into_iter().find(|m| m.id() == id)` with `metric_by_id(id)`.

- [ ] **Step 6: Run the tests and clippy**

Run: `cargo test -p ultros-app analyzer_kit::market` then `cargo clippy -p ultros-app --all-targets -- -D warnings`
Expected: PASS (5 tests in the module); no warnings. Routes calling `market.stats7()` (`routes/analyzer.rs:1343…`, `vendor_resale.rs:423…`, `venture_analyzer.rs:286`, `leve_analyzer.rs:258`, `fc_crafting_analyzer.rs:319`, `scrip_sources.rs:436`) compile untouched.

- [ ] **Step 7: CI check and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add ultros-frontend/ultros-app/src/analyzer_kit/market.rs
git commit -m "feat(analyzer-kit): sale-history columns for every server window, gil traded"
```

---

### Task 4: List the shared columns in the Flip Finder's Columns picker

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs:262-288` (helpers beside `serialize_visible_cols_preserving`), `:1417` (`visible_cols`), `:1657-1679` (`column_options`, `toggle_column`), `:2351-2352` (ControlBar props), tests near `:3817`.

**Interfaces:**
- Consumes: `market_picker_options()` and `STAT_COLUMNS` from Task 2.
- Produces: `fn toggle_shared_col(previous: Option<&str>, id: &str) -> String`, `fn shared_cols_in(raw: Option<&str>) -> HashSet<&'static str>`.

- [ ] **Step 1: Write the failing tests**

After `legacy_column_toggle_preserves_shared_and_provider_columns` (line 3840) add:

```rust
    #[test]
    fn shared_column_toggle_adds_then_removes_the_id() {
        // No `?cols=` means the default view, whose only shared column is
        // the sale estimate; ticking a stat column must keep it.
        let on = toggle_shared_col(None, "market-sale-median-7");
        assert_eq!(on, "sale_estimate,market-sale-median-7");
        let off = toggle_shared_col(Some(&on), "market-sale-median-7");
        assert_eq!(off, "sale_estimate");
        // A later native toggle keeps the shared id (the preserving path).
        let visible = parse_visible_cols(Some(&on));
        let serialized = serialize_visible_cols_preserving(&visible, Some(&on));
        assert!(serialized.split(',').any(|id| id == "market-sale-median-7"), "{serialized}");
    }

    #[test]
    fn picker_checked_state_reads_stat_ids_out_of_cols() {
        let set = shared_cols_in(Some("roi,market-sale-median-7,market-world,market-gil-30,bogus"));
        assert_eq!(
            set,
            std::collections::HashSet::from(["market-sale-median-7", "market-gil-30"])
        );
        assert!(shared_cols_in(None).is_empty());
    }
```

Run: `cargo test -p ultros-app routes::analyzer::tests::shared_column_toggle`
Expected: FAIL to compile.

- [ ] **Step 2: Add the two helpers**

Directly after `serialize_visible_cols_preserving` (line 288):

```rust
/// `?cols=` with one shared (non-native) id flipped, everything else kept
/// in place. Absent param = the default view, whose only shared column is
/// `sale_estimate` (see `serialize_visible_cols_preserving`).
fn toggle_shared_col(previous: Option<&str>, id: &str) -> String {
    let mut ids: Vec<&str> = previous
        .unwrap_or("sale_estimate")
        .split(',')
        .filter(|t| !t.is_empty())
        .collect();
    if let Some(i) = ids.iter().position(|t| *t == id) {
        ids.remove(i);
    } else {
        ids.push(id);
    }
    ids.join(",")
}

/// The stat-column ids present in `?cols=`, for the picker's checkboxes.
/// Native ids stay in `parse_visible_cols`; other shared ids (`market-world`)
/// are not picker entries and are ignored here.
fn shared_cols_in(raw: Option<&str>) -> std::collections::HashSet<&'static str> {
    raw.unwrap_or("")
        .split(',')
        .filter_map(|tok| {
            crate::analyzer_kit::stat_columns::STAT_COLUMNS
                .iter()
                .find(|c| c.id == tok)
                .map(|c| c.id)
        })
        .collect()
}
```

- [ ] **Step 3: Wire the picker**

After `let visible_cols = Memo::new(…)` (line 1417) add:

```rust
    // The toolbar picker also lists the shared sale-history columns; their
    // checked state lives in `?cols=` beside the native ids.
    let picker_visible = Memo::new(move |_| {
        let mut set = visible_cols.get();
        set.extend(shared_cols_in(cols_param().as_deref()));
        set
    });
```

Replace `column_options` (lines 1657-1662) with:

```rust
    // Columns the picker offers: the native columns in table order, then
    // every shared sale-history column grouped by window.
    let column_options = Memo::new(move |_| {
        let mut options = ALL_OPTIONAL_COLS
            .iter()
            .map(|col| ColumnOption::new(col, col_label(col)))
            .collect::<Vec<_>>();
        options.extend(crate::analyzer_kit::stat_columns::market_picker_options());
        options
    });
```

Replace `toggle_column` (lines 1668-1679) with:

```rust
    let toggle_column = Callback::new(move |col: &'static str| {
        let previous = cols_param.get_untracked();
        let mut set = visible_cols.get_untracked();
        let extras = if ALL_OPTIONAL_COLS.contains(&col) {
            if !set.remove(col) {
                set.insert(col);
            }
            previous
        } else {
            Some(toggle_shared_col(previous.as_deref(), col))
        };
        set_cols_param.set(Some(serialize_visible_cols_preserving(
            &set,
            extras.as_deref(),
        )));
    });
```

Change the ControlBar prop at line 2352 from `visible_columns=visible_cols` to `visible_columns=picker_visible`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-app routes::analyzer::tests`
Expected: all PASS, including `default_columns_are_everything_but_tax_and_volume` (ALL_OPTIONAL_COLS is unchanged) and `legacy_column_toggle_preserves_shared_and_provider_columns`.

- [ ] **Step 5: Look at it in a browser**

Use the `run` skill / `.claude/launch.json` to serve the branch (LEPTOS_FEATURES / env per `AGENTS.md`; memory: `reference_ultros_local_browser_test`). Open `/flip-finder/Gilgamesh`, click **Columns**:

- The 11 native entries come first with no heading; then headings "Sale history (1d)", "(7d)", "(30d)", "(90d)", nine entries each.
- Tick "Sale median (7d)": URL gains `cols=…,sale_estimate,market-sale-median-7`, a "Sale median (7d)" header appears at the right edge, checkbox shows ticked on reopen. Untick: id leaves the URL, column hides.
- Tick "Gil traded (30d)": a `sale_stats/Gilgamesh?window=30` request fires (Network tab) and the cell fills — on prod; locally the body may stay "Loading…" (known environment trap), which is not a regression.
- Open the header ⋮ on "Sale median (7d)" → **Hide column**: the picker unticks it.
- At 375px width the control bar still fits (the popover is a dropdown, row 1 is unchanged).

- [ ] **Step 6: CI check and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "feat(flip-finder): list shared sale-history columns in the Columns picker"
```

---

### Task 5: E2E coverage and changelog

**Files:**
- Modify: `integration/shared-analyzer-data.cjs:200-201`
- Create: `ultros-changelog/changes/2026-09-07-sale-history-windows.json`

- [ ] **Step 1: Assert the new ids register on every analyzer**

Change the `shared` list to:

```js
      const shared = ['market-sale-median-7', 'market-sale-min-7', 'market-sale-avg-7',
        'market-sale-median-30', 'market-gil-7',
        'market-world', 'market-datacenter', 'market-sales-per-day-7', 'market-cadence-7', 'market-trend-7'];
```

`medianColumn` stays `required[0]`, so the fixture's value wait is unchanged; the fixture replies the same `stats` body for any `window=` (`integration/shared-analyzer-market-fixture.cjs:38`).

- [ ] **Step 2: Run the probe**

Run: `CHECK_ANALYZER_ROUTES=1 ANALYZER_TOOLS=flip-finder,venture-analyzer ./scripts/run_e2e.sh` (see `AGENTS.md`; needs the local server from Task 4 or `BASE_URL`).
Expected: `flip-finder registers market-sale-median-30` and `… market-gil-7` assertions pass; recipe-analyzer is unaffected (its own `required` list).

- [ ] **Step 3: Changelog entry**

```json
{
  "category": "improvements",
  "importance": "medium",
  "title": "Sale history for every window, right in the Columns picker",
  "blurb": "Every analyzer can now show sale minimum, median, average, units, sales, VWAP and total gil traded over the last day, week, month or 90 days. The Flip Finder lists them all in its Columns menu, grouped by window.",
  "link": "/flip-finder"
}
```

- [ ] **Step 4: CI check and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add integration/shared-analyzer-data.cjs ultros-changelog/changes/2026-09-07-sale-history-windows.json docs/superpowers/plans/2026-09-07-flip-finder-sale-metrics.md
git commit -m "test(e2e): cover 30d and gil sale-history columns; changelog"
```

---

## Verification (end to end)

1. `./check_ci.sh` green on the final branch (fmt, clippy, unit tests).
2. Optional but valuable: ClickHouse smoke with a throwaway container (`ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test sale_stats_smoke`) — the only test that executes the modified SQL; SQL-string tests cannot see runtime alias errors (memory: `reference_clickhouse_alias_shadows_column`).
3. Local browser pass per Task 4 Step 5, plus `/venture-analyzer` header ⋮ → Insert column after… → search "gil" lists "Gil traded (7d)" through "(90d)".
4. After deploy, on prod:
   - `curl -s 'https://ultros.app/api/v1/sale_stats/Gilgamesh?window=1' | jq '.stats[0]'` shows `gil_volume` > 0.
   - `/flip-finder/Gilgamesh?cols=profit_per_day,market-sale-median-30,market-gil-90` renders both headers with numbers, and the Network tab shows exactly `sale_stats?window=7`, `window=30` and `window=90` (no `window=1`).
   - `/flip-finder/Gilgamesh` with no `cols` fires only `window=7` (default behaviour unchanged).
   - Existing bookmark `?cols=…,market-units-30` still renders "Units sold (30d)" (id contract).

## Self-review notes

- Spec coverage: picker (Task 4), extra windows for the full set (Tasks 2–3), gil volume (Task 1 + `StatKind::GilVolume`), LOE table (top), e2e + changelog (Task 5). Default-on / position / native sort explicitly declined by the user.
- Names used across tasks: `Window`, `StatKind`, `STAT_COLUMNS`, `stat_column`, `stat_label`, `window_wanted`, `market_picker_options` (Task 2) ↔ Task 3 `use` line and Task 4 paths; `MarketData::stats/stats_failed/want/track_all` (Task 3 only); `toggle_shared_col`, `shared_cols_in`, `picker_visible` (Task 4). `ItemSaleStats.gil_volume` (Task 1) ↔ Task 3 `StatKind::GilVolume`.
