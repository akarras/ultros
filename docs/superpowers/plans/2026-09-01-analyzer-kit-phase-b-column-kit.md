# Analyzer Kit — Phase B: Column Kit and Recipe Table Adoption — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the recipe analyzer's hand-written header and cell blocks with a static column table rendered by a shared grid, with the same pixels, the same numbers and the same URL contract, as one PR against `main` after Phase A merges.

**Architecture:** `analyzer_kit/columns.rs` defines a page-independent, closure-free `ColumnSpec` and a per-page `ToolColumnMeta<T, M>` (URL token, sort token, classes, default-on, a plain `fn` cell extractor) so a page's whole column table is a `static`; `?cols=` order, the picker, and every `SortMode` token derive from it. `analyzer_kit/cells.rs` renders a small `CellValue` enum with one DOM shape per variant. `analyzer_kit/grid.rs` hosts header and rows over the untouched `VirtualScroller`, reading `visible_cols` once per row. Cells that read page context (Item, Cost / unit, Actions, listing World/DC, Daily sales with its tooltip) stay page-owned through a `custom` escape hatch. Spec: `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md` sections 3 and 5, Phase B.

**Tech Stack:** Rust 2024, Leptos 0.8 (`VirtualScroller`, `Memo`, `Signal`, `AnyView`), leptos_i18n 0.6 (`t_string!` inside `fn(I18nContext) -> String` pointers).

## Global Constraints

- Requires Phase A merged (`analyzer_kit::{formula, signals, needed}`, `price_rows`, `filter_and_sort`, page-level `sort_mode`/`sort_dir` props). Open this PR against `main` and `rebase --onto` if A is still open.
- `./check_ci.sh` clean before the PR; `cargo test -p ultros-app --lib` green locally (CI runs no tests).
- Every module is `pub(crate)`; introduce only what this phase consumes. No `#[allow(dead_code)]`.
- Zero number change (the Phase A oracle `price_rows_matches_recorded_oracle_on_fixture` stays green untouched) and zero URL change: the seven `?cols=` tokens `confidence, last-sold, volume, vwap, tax, listing-world, listing-dc`, `DEFAULT_COLS = [confidence]`, and the eleven `?sort=` tokens keep parsing and serializing identically.
- Same pixels: every cell keeps its class string verbatim (`px-4 py-2 w-32 shrink-0 text-right`, `hidden md:block`, …). The one deliberate change is the VWAP cell, which stops switching between two element shapes (a hydration hazard): rows with a VWAP are pixel-identical; rows without one now render the dash through `GilOrDash`'s flex row (left-aligned, like the `Gil` amounts above it) instead of a right-aligned inline span, plus an empty sub-line.
- No changelog entry; no new i18n keys.
- Do not touch `VirtualScroller`, `data_table.rs` or `skeleton.rs`.
- Line numbers below are against `main` before Phase A merges; Phase A moves them, so every edit also carries a textual anchor to grep for. Trust the anchor.

---

## File map

| File | Responsibility |
|---|---|
| `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs` (new) | `ColumnKind`, `ColumnSpec`, `LabelFn`, `Layer`, `Sortability`, `sortability_for`, `CellCtx`, `ToolColumnMeta`, derivations (`picker_options`, `sort_token`, `sort_from_token`, `default_dir_for`) |
| `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs` (new) | `CellValue`, `render_cell`, `last_sold_label` (moved from the route) |
| `ultros-frontend/ultros-app/src/analyzer_kit/grid.rs` (new) | `AnalyzerRow`, `GridLayout`, `AnalyzerGrid` |
| `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs` (modify) | register the three modules |
| `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (modify) | `RECIPE_COLUMNS` static, `impl AnalyzerRow`, cell/label fns, `custom` cells, `SortMode` delegation; header block (1520-1622) and row block (1623-1852) deleted |
| `ultros-frontend/ultros-app/src/components/control_bar.rs` (modify) | `ColumnOption::new` constructor |

---

### Task 1: `columns.rs` — the closure-free column table

**Files:**
- Create: `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs`
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs` (add `pub mod columns;`)
- Modify: `ultros-frontend/ultros-app/src/components/control_bar.rs:44-49` (add a constructor)

**Interfaces:**
- Consumes: `ColumnOption` (`control_bar.rs`), `SortColumn`, `SortDir` (`sort_header.rs`), `I18nContext<Locale, I18nKeys>`.
- Produces:
  - `pub type LabelFn = fn(I18nContext<Locale, I18nKeys>) -> String;`
  - `pub enum ColumnKind { Item, Profit, Roi, CostSlot, RevenueSlot, SalesPerDay7, AvgPrice, Confidence, LastSold, VolumeUnits7, Vwap7, Tax, ListingWorld, ListingDc, Actions }`
  - `pub struct ColumnSpec { pub kind: ColumnKind, pub label: LabelFn }` (`canonical_id` and the `CATALOG` array arrive in Phase G with their first reader: a constructed-but-never-read field fails `-D warnings`)
  - `pub enum Layer { RowLocal, Computed, Bulk }`, `pub enum Sortability<M> { No, By(M) }`, `pub const fn sortability_for<M: Copy>(layer: Layer, wanted: Option<M>) -> Sortability<M>`
  - `pub struct CellCtx { pub now_unix: i64 }`
  - `pub struct ToolColumnMeta<T: 'static, M: 'static> { pub spec: &'static ColumnSpec, pub id: &'static str, pub sort_id: &'static str, pub sort: Sortability<M>, pub default_dir: SortDir, pub header_class: &'static str, pub cell_class: &'static str, pub default_on: bool, pub cell: fn(&T, &CellCtx) -> CellValue }`
  - `picker_options(cols, i18n) -> Vec<ColumnOption>`, `sort_token(cols, m) -> Option<&'static str>`, `sort_from_token(cols, s) -> Option<M>`, `default_dir_for(cols, m) -> SortDir`
  - `ColumnOption::new(id: &'static str, label: String) -> ColumnOption`

- [ ] **Step 1: Write the failing tests** (create `columns.rs` with the test module; the fixture uses a tiny local `Col` enum so the test does not depend on the route)

```rust
// ultros-frontend/ultros-app/src/analyzer_kit/columns.rs
//! A page's column table as data. `ColumnSpec` is page-independent;
//! `ToolColumnMeta` binds a spec to one page's URL token, sort token,
//! classes and cell extractor. The whole table is a `static`, so the
//! context-free `FromStr`/`Display` impls on a page's `SortMode` and the
//! `&'static` id slices `parse_visible_cols` needs can read it.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::sort_header::SortColumn;
    use std::fmt;

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    enum Col {
        Profit,
        Cost,
    }
    impl fmt::Display for Col {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(sort_token(&COLS, *self).unwrap_or("profit"))
        }
    }
    impl SortColumn for Col {
        fn fallback() -> Self {
            Col::Profit
        }
        fn default_dir(self) -> SortDir {
            default_dir_for(&COLS, self)
        }
    }

    fn label_item(_: I18nContext<Locale, I18nKeys>) -> String {
        "Item".into()
    }
    fn label_profit(_: I18nContext<Locale, I18nKeys>) -> String {
        "Profit".into()
    }
    fn label_cost(_: I18nContext<Locale, I18nKeys>) -> String {
        "Cost".into()
    }
    static SPEC_ITEM: ColumnSpec = ColumnSpec { kind: ColumnKind::Item, label: label_item };
    static SPEC_PROFIT: ColumnSpec = ColumnSpec { kind: ColumnKind::Profit, label: label_profit };
    static SPEC_COST: ColumnSpec = ColumnSpec { kind: ColumnKind::CostSlot, label: label_cost };

    fn no_cell(_: &i32, _: &CellCtx) -> CellValue {
        CellValue::Custom
    }
    fn gil_cell(v: &i32, _: &CellCtx) -> CellValue {
        CellValue::Gil(*v)
    }

    static COLS: [ToolColumnMeta<i32, Col>; 3] = [
        ToolColumnMeta { spec: &SPEC_ITEM, id: "", sort_id: "", sort: Sortability::No, default_dir: SortDir::Desc, header_class: "w-64", cell_class: "w-64", default_on: true, cell: no_cell },
        ToolColumnMeta { spec: &SPEC_PROFIT, id: "", sort_id: "profit", sort: sortability_for(Layer::Computed, Some(Col::Profit)), default_dir: SortDir::Desc, header_class: "w-32", cell_class: "w-32", default_on: true, cell: gil_cell },
        ToolColumnMeta { spec: &SPEC_COST, id: "cost", sort_id: "cost", sort: sortability_for(Layer::Computed, Some(Col::Cost)), default_dir: SortDir::Asc, header_class: "w-32", cell_class: "w-32", default_on: false, cell: gil_cell },
    ];

    #[test]
    fn ids_and_defaults_come_from_the_table_in_order() {
        // Derived inline: a kit fn with only test callers is dead code.
        let ids: Vec<&str> = COLS.iter().filter(|c| !c.id.is_empty()).map(|c| c.id).collect();
        assert_eq!(ids, vec!["cost"]);
        let defaults: Vec<&str> = COLS.iter().filter(|c| !c.id.is_empty() && c.default_on).map(|c| c.id).collect();
        assert_eq!(defaults, Vec::<&str>::new());
    }

    #[test]
    fn sort_tokens_round_trip_and_fall_back() {
        assert_eq!(sort_token(&COLS, Col::Profit), Some("profit"));
        assert_eq!(sort_from_token(&COLS, "cost"), Some(Col::Cost));
        assert_eq!(sort_from_token(&COLS, "bogus"), None);
        assert_eq!(default_dir_for(&COLS, Col::Cost), SortDir::Asc);
        assert_eq!(default_dir_for(&COLS, Col::Profit), SortDir::Desc);
        assert_eq!(Col::Cost.to_string(), "cost");
    }

    #[test]
    fn sortability_follows_the_layer() {
        assert_eq!(sortability_for(Layer::RowLocal, Some(Col::Profit)), Sortability::By(Col::Profit));
        assert_eq!(sortability_for(Layer::Bulk, Some(Col::Profit)), Sortability::By(Col::Profit));
        assert_eq!(sortability_for(Layer::Computed, None::<Col>), Sortability::No);
    }

    #[test]
    fn cell_extractors_are_plain_fn_pointers() {
        let ctx = CellCtx { now_unix: 0 };
        assert_eq!((COLS[1].cell)(&42, &ctx), CellValue::Gil(42));
        assert_eq!((COLS[0].cell)(&42, &ctx), CellValue::Custom);
    }
}
```

- [ ] **Step 2: Register and run to verify failure**

Add `pub mod columns;` to `analyzer_kit/mod.rs`. Run: `cargo test -p ultros-app --lib analyzer_kit::columns`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
use leptos_i18n::I18nContext;

use crate::components::control_bar::ColumnOption;
use crate::components::sort_header::{SortColumn, SortDir};
use crate::i18n::*;

use super::cells::CellValue;

/// A label resolver. A plain `fn` so a column table can be a `static`;
/// the page resolves it inside a reactive closure so headers follow the
/// locale.
pub type LabelFn = fn(I18nContext<Locale, I18nKeys>) -> String;

/// What a column IS, independent of the page showing it. Kinds name a
/// definition, not a label: a 7-day unit volume and a 30-day one are
/// different kinds.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ColumnKind {
    Item,
    Profit,
    Roi,
    CostSlot,
    RevenueSlot,
    SalesPerDay7,
    AvgPrice,
    Confidence,
    LastSold,
    VolumeUnits7,
    Vwap7,
    Tax,
    ListingWorld,
    ListingDc,
    Actions,
}

/// Page-independent, closure-free description of a column.
pub struct ColumnSpec {
    /// Read by the grid to route `CellValue::Custom` cells to the page.
    pub kind: ColumnKind,
    pub label: LabelFn,
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
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Sortability<M> {
    No,
    By(M),
}

pub const fn sortability_for<M: Copy>(layer: Layer, wanted: Option<M>) -> Sortability<M> {
    match (layer, wanted) {
        (Layer::RowLocal | Layer::Computed | Layer::Bulk, Some(m)) => Sortability::By(m),
        (_, None) => Sortability::No,
    }
}

/// Per-render context a cell extractor may read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CellCtx {
    pub now_unix: i64,
}

/// One page's binding of a [`ColumnSpec`]: its `?cols=` token (`""` for
/// an always-on column), `?sort=` token (`""` when unsortable), classes
/// copied verbatim from the page's markup, and a `fn` pointer extracting
/// the cell value from a row. Everything URL-facing lives here so the
/// page's `SortMode` impls and `parse_visible_cols` can read a `static`.
pub struct ToolColumnMeta<T: 'static, M: 'static> {
    pub spec: &'static ColumnSpec,
    pub id: &'static str,
    pub sort_id: &'static str,
    pub sort: Sortability<M>,
    pub default_dir: SortDir,
    pub header_class: &'static str,
    pub cell_class: &'static str,
    pub default_on: bool,
    pub cell: fn(&T, &CellCtx) -> CellValue,
}

pub fn picker_options<T, M>(
    cols: &'static [ToolColumnMeta<T, M>],
    i18n: I18nContext<Locale, I18nKeys>,
) -> Vec<ColumnOption> {
    cols.iter()
        .filter(|c| !c.id.is_empty())
        .map(|c| ColumnOption::new(c.id, (c.spec.label)(i18n)))
        .collect()
}

pub fn sort_token<T, M: SortColumn>(cols: &'static [ToolColumnMeta<T, M>], m: M) -> Option<&'static str> {
    cols.iter()
        .find(|c| matches!(c.sort, Sortability::By(x) if x == m))
        .map(|c| c.sort_id)
}

pub fn sort_from_token<T, M: SortColumn>(cols: &'static [ToolColumnMeta<T, M>], s: &str) -> Option<M> {
    cols.iter().find(|c| !c.sort_id.is_empty() && c.sort_id == s).and_then(|c| match c.sort {
        Sortability::By(m) => Some(m),
        Sortability::No => None,
    })
}

pub fn default_dir_for<T, M: SortColumn>(cols: &'static [ToolColumnMeta<T, M>], m: M) -> SortDir {
    cols.iter()
        .find(|c| matches!(c.sort, Sortability::By(x) if x == m))
        .map(|c| c.default_dir)
        .unwrap_or(SortDir::Desc)
}
```

And in `control_bar.rs`, under the `ColumnOption` struct:

```rust
impl ColumnOption {
    pub fn new(id: &'static str, label: String) -> Self {
        Self { id, label }
    }
}
```

`cells.rs` does not exist yet, so Task 2's `CellValue` must be created before this compiles: create `cells.rs` now with just the enum from Task 2 Step 3 (the `render_cell` function comes with Task 2's tests). Register `pub mod cells;` in `mod.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-app --lib analyzer_kit::columns`
Expected: PASS (4 tests). Clippy reports everything in `analyzer_kit::{columns, cells}` as dead until Task 4 wires the route; do not add `#[allow]` — the PR-level `./check_ci.sh` in Task 4 Step 9 is the gate.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-app/src/analyzer_kit ultros-frontend/ultros-app/src/components/control_bar.rs
git commit -m "feat(analyzer-kit): static column tables (ColumnSpec, ToolColumnMeta, derivations)"
```

---

### Task 2: `cells.rs` — one renderer, one shape per variant

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:96-114` (move `last_sold_label` out)

**Interfaces:**
- Consumes: `Gil`, `GilOrDash` (`components/gil.rs`), `ConfidenceBadge`, `roi_badge_class` (`analysis.rs`), `CellCtx`.
- Produces:
  - `pub enum CellValue { Gil(i32), RoiBadge(i32), Count(u64), Confidence(ConfidenceBand), LastSoldUnix(i64), GilWithPct { amount: i32, pct: Option<f32> }, Custom }`
  - `pub fn render_cell(class: &'static str, value: CellValue, i18n: I18nContext<Locale, I18nKeys>, ctx: &CellCtx) -> Option<AnyView>` (`None` for `Custom`)
  - `pub fn last_sold_label(i18n, last_sold_unix: i64, now_unix: i64) -> String` (moved)

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::*;
    use leptos_i18n::context::init_i18n_context;

    fn count(html: &str, needle: &str) -> usize {
        html.matches(needle).count()
    }

    /// Each resource-backed variant keeps one element shape between its
    /// value and no-value states (the `GilOrDash` rule): SSR and CSR must
    /// agree on tags even when a payload lands late.
    #[test]
    fn render_cell_keeps_one_shape_per_variant() {
        // `<Gil>` calls the panicking `use_i18n()`, and building an
        // I18nContext spawns an Effect: stand up both, as
        // components/list/filter_row.rs's tests do.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let ctx = CellCtx { now_unix: 1_700_000_000 };
            let a = render_cell("w-32", CellValue::GilWithPct { amount: 120, pct: Some(4.2) }, i18n, &ctx).unwrap().to_html();
            let b = render_cell("w-32", CellValue::GilWithPct { amount: 0, pct: None }, i18n, &ctx).unwrap().to_html();
            assert_eq!(count(&a, "role=\"cell\""), 1);
            assert_eq!(count(&a, "<div"), count(&b, "<div"), "{a}\n{b}");
            assert!(a.contains("+4%"), "{a}");
            assert!(b.contains("—"), "{b}");
            let never = render_cell("w-28", CellValue::LastSoldUnix(0), i18n, &ctx).unwrap().to_html();
            let recent = render_cell("w-28", CellValue::LastSoldUnix(1_699_999_000), i18n, &ctx).unwrap().to_html();
            assert_eq!(count(&never, "<div"), count(&recent, "<div"));
            assert!(render_cell("w-32", CellValue::Custom, i18n, &ctx).is_none());
        });
    }

    #[test]
    fn last_sold_label_buckets() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let now = 1_700_000_000;
            assert!(!last_sold_label(i18n, 0, now).is_empty());
            let two_days = last_sold_label(i18n, now - 2 * 86_400, now);
            assert!(two_days.contains('2'), "{two_days}");
            let three_hours = last_sold_label(i18n, now - 3 * 3_600, now);
            assert!(three_hours.contains('3'), "{three_hours}");
        });
    }
}
```

The executor + `provide_context(init_i18n_context…)` preamble is the pattern from `components/list/filter_row.rs`'s `filter_row_renders_all_groups` test; `sort_header.rs`'s owner-only test is not enough here because `<Gil>` reads the i18n context. Assert on structure (tag counts) rather than on English words where the text is locale-dependent.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros-app --lib analyzer_kit::cells`
Expected: FAIL to compile (`render_cell` missing).

- [ ] **Step 3: Implement**

```rust
//! The kit's cell vocabulary: a small value enum rendered by one match,
//! so per-variant markup lives in exactly one place and every
//! resource-backed variant keeps one DOM shape across its states.

use leptos::prelude::*;
use leptos_i18n::I18nContext;
use ultros_api_types::trends::ConfidenceBand;

use crate::analysis::roi_badge_class;
use crate::components::confidence_badge::ConfidenceBadge;
use crate::components::gil::{Gil, GilOrDash};
use crate::i18n::*;

use super::columns::CellCtx;

#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Gil(i32),
    RoiBadge(i32),
    Count(u64),
    Confidence(ConfidenceBand),
    LastSoldUnix(i64),
    /// A gil amount with a percent sub-line (VWAP and its % vs price).
    /// `amount <= 0` renders the dash; the sub-line is always present.
    GilWithPct { amount: i32, pct: Option<f32> },
    /// The page renders this cell itself.
    Custom,
}

/// Resting label for a last-sold cell: day / hour / just-now buckets. A
/// zero or future timestamp renders as "never".
pub fn last_sold_label(i18n: I18nContext<Locale, I18nKeys>, last_sold_unix: i64, now_unix: i64) -> String {
    if last_sold_unix <= 0 || last_sold_unix > now_unix {
        return t_string!(i18n, analyzer_last_sold_never).to_string();
    }
    let secs = (now_unix - last_sold_unix) as u64;
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    if days > 0 {
        t_string!(i18n, analyzer_last_sold_days_ago).replace("%count%", &days.to_string())
    } else if hours > 0 {
        t_string!(i18n, analyzer_last_sold_hours_ago).replace("%count%", &hours.to_string())
    } else {
        t_string!(i18n, analyzer_last_sold_just_now).to_string()
    }
}

/// Render one cell. `None` for [`CellValue::Custom`]; the host asks the
/// page for those.
pub fn render_cell(
    class: &'static str,
    value: CellValue,
    i18n: I18nContext<Locale, I18nKeys>,
    ctx: &CellCtx,
) -> Option<AnyView> {
    Some(match value {
        CellValue::Gil(amount) => view! {
            <div role="cell" class=class><Gil amount=amount /></div>
        }
        .into_any(),
        CellValue::RoiBadge(roi) => view! {
            <div role="cell" class=class>
                <span class=roi_badge_class(roi)>{format!("{roi}%")}</span>
            </div>
        }
        .into_any(),
        CellValue::Count(n) => view! {
            <div role="cell" class=class>{n.to_string()}</div>
        }
        .into_any(),
        CellValue::Confidence(band) => view! {
            <div role="cell" class=class><ConfidenceBadge band=band /></div>
        }
        .into_any(),
        CellValue::LastSoldUnix(unix) => {
            let label = last_sold_label(i18n, unix, ctx.now_unix);
            view! { <div role="cell" class=class>{label}</div> }.into_any()
        }
        CellValue::GilWithPct { amount, pct } => {
            let sub = pct.filter(|_| amount > 0).map(|p| format!("{p:+.0}%")).unwrap_or_default();
            view! {
                <div role="cell" class=class>
                    <GilOrDash amount=(amount > 0).then_some(amount) />
                    <div class="text-xs text-[color:var(--color-text-muted)]">{sub}</div>
                </div>
            }
            .into_any()
        }
        CellValue::Custom => return None,
    })
}
```

Delete `last_sold_label` from `recipe_analyzer.rs` (the `/// Resting label for the last-sold cell` doc comment through the fn's closing brace, lines 92-115 today) and import it from the kit where the route still calls it (it will not after Task 4; leave the import until then).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-app --lib analyzer_kit::cells`
Expected: PASS (2 tests). Clippy's dead-code wall persists until Task 4 — proceed.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-app/src/analyzer_kit/cells.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(analyzer-kit): CellValue and the shape-constant render_cell"
```

---

### Task 3: `grid.rs` — the host over the untouched `VirtualScroller`

**Files:**
- Create: `ultros-frontend/ultros-app/src/analyzer_kit/grid.rs`
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs` (add `pub mod grid;`)

**Interfaces:**
- Consumes: `ToolColumnMeta`, `Sortability`, `CellCtx`, `render_cell`, `CellValue`, `SortableHeaderCell`, `SortColumn`, `SortDir`, `VirtualScroller`.
- Produces:
  - `pub trait AnalyzerRow: Clone + Send + Sync + PartialEq + 'static { type Key: Eq + Hash + 'static; fn key(&self) -> Self::Key; }`
  - `#[derive(Copy, Clone)] pub struct GridLayout { pub viewport_height: f64, pub row_height: f64, pub header_height: f64, pub overscan: u32 }`
  - `#[component] pub fn AnalyzerGrid<T: AnalyzerRow, M: SortColumn>(columns: &'static [ToolColumnMeta<T, M>], rows: Signal<Vec<(usize, T)>>, visible_cols: Signal<HashSet<&'static str>>, sort_mode: Signal<Option<M>>, sort_dir: Signal<Option<SortDir>>, ctx: Signal<CellCtx>, custom: Arc<dyn Fn(&T, ColumnKind) -> AnyView + Send + Sync>, layout: GridLayout, header_class: &'static str, row_class: fn(usize) -> &'static str) -> impl IntoView`

- [ ] **Step 1: Write the failing test** (an SSR render under an owner: header cells and one row's cells)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer_kit::columns::{ColumnKind, ColumnSpec, Layer, sortability_for};
    use leptos_i18n::context::init_i18n_context;
    use std::fmt;

    #[derive(Clone, PartialEq)]
    struct Row(i32);
    impl AnalyzerRow for Row {
        type Key = i32;
        fn key(&self) -> i32 {
            self.0
        }
    }
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    enum Col {
        Profit,
    }
    impl fmt::Display for Col {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("profit")
        }
    }
    impl SortColumn for Col {
        fn fallback() -> Self {
            Col::Profit
        }
    }
    fn label_a(_: I18nContext<Locale, I18nKeys>) -> String {
        "Item".into()
    }
    fn label_b(_: I18nContext<Locale, I18nKeys>) -> String {
        "Profit".into()
    }
    fn label_c(_: I18nContext<Locale, I18nKeys>) -> String {
        "Extra".into()
    }
    static A: ColumnSpec = ColumnSpec { kind: ColumnKind::Item, label: label_a };
    static B: ColumnSpec = ColumnSpec { kind: ColumnKind::Profit, label: label_b };
    static C: ColumnSpec = ColumnSpec { kind: ColumnKind::Tax, label: label_c };
    fn custom_cell(_: &Row, _: &CellCtx) -> CellValue {
        CellValue::Custom
    }
    fn gil(r: &Row, _: &CellCtx) -> CellValue {
        CellValue::Gil(r.0)
    }
    static COLS: [ToolColumnMeta<Row, Col>; 3] = [
        ToolColumnMeta { spec: &A, id: "", sort_id: "", sort: Sortability::No, default_dir: SortDir::Desc, header_class: "w-64", cell_class: "w-64", default_on: true, cell: custom_cell },
        ToolColumnMeta { spec: &B, id: "", sort_id: "profit", sort: sortability_for(Layer::Computed, Some(Col::Profit)), default_dir: SortDir::Desc, header_class: "w-32", cell_class: "w-32", default_on: true, cell: gil },
        ToolColumnMeta { spec: &C, id: "extra", sort_id: "", sort: Sortability::No, default_dir: SortDir::Desc, header_class: "w-28", cell_class: "w-28", default_on: false, cell: gil },
    ];
    fn stripe(_: usize) -> &'static str {
        "row"
    }

    #[test]
    fn grid_renders_visible_columns_only() {
        // The Profit cell renders `<Gil>`, which reads the i18n context.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let visible = RwSignal::new(HashSet::<&'static str>::new());
            let html = view! {
                <AnalyzerGrid
                    columns=&COLS
                    rows=Signal::derive(|| vec![(0usize, Row(7))])
                    visible_cols=visible
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    ctx=Signal::derive(|| CellCtx { now_unix: 0 })
                    custom=Arc::new(|r: &Row, kind: ColumnKind| view! { <div role="cell" class="w-64">{format!("custom {kind:?} {}", r.0)}</div> }.into_any())
                    layout=GridLayout { viewport_height: 720.0, row_height: 60.0, header_height: 64.0, overscan: 8 }
                    header_class="thead"
                    row_class=stripe
                />
            }
            .to_html();
            assert!(html.contains("custom Item 7"), "{html}");
            assert!(html.contains("Profit"), "{html}");
            assert!(!html.contains("Extra"), "{html}");
            assert_eq!(html.matches("role=\"cell\"").count(), 2, "{html}");
        });
    }
}
```

- [ ] **Step 2: Register and run to verify failure**

Add `pub mod grid;` to `mod.rs`. Run: `cargo test -p ultros-app --lib analyzer_kit::grid`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
//! The table host: header and rows rendered from a page's static column
//! table over the existing `VirtualScroller`, which needs no changes.
//! Visibility derives only from `?cols=` (URL-borne, identical on server
//! and client) and is read once per row, replacing one gate closure per
//! optional cell per row.

use std::collections::HashSet;
use std::hash::Hash;
use std::sync::Arc;

use leptos::prelude::*;
use leptos_i18n::I18nContext;

use crate::components::sort_header::{SortColumn, SortDir, SortableHeaderCell};
use crate::components::virtual_scroller::VirtualScroller;
use crate::i18n::*;

use super::cells::{CellValue, render_cell};
use super::columns::{CellCtx, ColumnKind, Sortability, ToolColumnMeta};

pub trait AnalyzerRow: Clone + Send + Sync + PartialEq + 'static {
    type Key: Eq + Hash + 'static;
    fn key(&self) -> Self::Key;
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GridLayout {
    pub viewport_height: f64,
    pub row_height: f64,
    pub header_height: f64,
    pub overscan: u32,
}

fn header_cell<T: 'static, M: SortColumn>(
    col: &'static ToolColumnMeta<T, M>,
    sort_mode: Signal<Option<M>>,
    sort_dir: Signal<Option<SortDir>>,
    i18n: I18nContext<Locale, I18nKeys>,
) -> AnyView {
    let label_fn = col.spec.label;
    match col.sort {
        Sortability::By(mode) => view! {
            <SortableHeaderCell mode=mode label=label_fn(i18n) class=col.header_class sort_mode sort_dir />
        }
        .into_any(),
        // Unsortable headers were `t!(..)` on the page (locale-reactive);
        // keep that by resolving the label inside a closure.
        Sortability::No => view! {
            <div role="columnheader" class=col.header_class>{move || label_fn(i18n)}</div>
        }
        .into_any(),
    }
}

#[component]
pub fn AnalyzerGrid<T: AnalyzerRow, M: SortColumn>(
    columns: &'static [ToolColumnMeta<T, M>],
    #[prop(into)] rows: Signal<Vec<(usize, T)>>,
    #[prop(into)] visible_cols: Signal<HashSet<&'static str>>,
    #[prop(into)] sort_mode: Signal<Option<M>>,
    #[prop(into)] sort_dir: Signal<Option<SortDir>>,
    #[prop(into)] ctx: Signal<CellCtx>,
    /// Renders the cells whose extractor returned [`CellValue::Custom`],
    /// keyed by the column's kind (always-on columns have no `id`).
    custom: Arc<dyn Fn(&T, ColumnKind) -> AnyView + Send + Sync>,
    layout: GridLayout,
    header_class: &'static str,
    row_class: fn(usize) -> &'static str,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();

    let header = view! {
        <div class=header_class role="rowgroup">
            {columns
                .iter()
                .map(|col| {
                    if col.id.is_empty() {
                        header_cell(col, sort_mode, sort_dir, i18n)
                    } else {
                        (move || {
                            visible_cols
                                .get()
                                .contains(col.id)
                                .then(|| header_cell(col, sort_mode, sort_dir, i18n))
                        })
                        .into_any()
                    }
                })
                .collect_view()}
        </div>
    }
    .into_any();

    view! {
        <VirtualScroller
            viewport_height=layout.viewport_height
            row_height=layout.row_height
            overscan=layout.overscan
            header_height=layout.header_height
            variable_height=false
            header=header
            each=rows
            key=move |(index, row): &(usize, T)| (*index, row.key())
            view=move |(index, row): (usize, T)| {
                let custom = custom.clone();
                view! {
                    <div class=row_class(index) role="row-group">
                        {move || {
                            let vis = visible_cols.get();
                            let c = ctx.get();
                            columns
                                .iter()
                                .filter(|col| col.id.is_empty() || vis.contains(col.id))
                                .map(|col| match (col.cell)(&row, &c) {
                                    CellValue::Custom => custom(&row, col.spec.kind),
                                    value => render_cell(col.cell_class, value, i18n, &c)
                                        .expect("only Custom renders None"),
                                })
                                .collect_view()
                        }}
                    </div>
                }
            }
        />
    }
}
```

Note: `custom` is keyed by `col.spec.kind`, so always-on custom columns (`id == ""`) are distinguishable and `ColumnSpec.kind` has its reader (a field that is only constructed is dead code under `-D warnings`). If the `#[component]` macro rejects the `&'static [ToolColumnMeta<T, M>]` prop, wrap it: `columns: &'static [ToolColumnMeta<T, M>]` → `#[prop(into)] columns: Columns<T, M>` where `pub struct Columns<T: 'static, M: 'static>(pub &'static [ToolColumnMeta<T, M>]);` with `From<&'static [..]>`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-app --lib analyzer_kit::grid`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-app/src/analyzer_kit
git commit -m "feat(analyzer-kit): AnalyzerGrid host over VirtualScroller"
```

---

### Task 4: Recipe analyzer adopts the kit (same pixels, less code)

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` — after the column registry (insert after `const DEFAULT_COLS`), `col_label`/`column_options` (from `// Optional-column picker, flip-finder style.` to the `});` closing `column_options`), the `SortMode` impls (`impl FromStr for SortMode` through `impl SortColumn for SortMode`), the whole table block (`<div class="rounded-2xl overflow-x-auto panel …">` through its matching `</div>`)

**Interfaces:**
- Consumes: everything from Tasks 1-3; `price_rows`/`filter_and_sort`/`computed_data` from Phase A.
- Produces (route-private): `static RECIPE_COLUMNS: [ToolColumnMeta<Arc<RecipeProfitData>, SortMode>; 15]`, `impl AnalyzerRow for Arc<RecipeProfitData>`, cell and label fns, the `custom` renderer closure. (`OPTIONAL_COLUMN_ORDER` / `DEFAULT_COLS` stay hand-written and are pinned equal to the table by test.)

- [ ] **Step 1: Write the failing URL-contract tests** (append to `mod test`)

```rust
    #[test]
    fn recipe_optional_column_order_is_a_stable_url_contract() {
        assert_eq!(
            OPTIONAL_COLUMN_ORDER,
            &["confidence", "last-sold", "volume", "vwap", "tax", "listing-world", "listing-dc"]
        );
        assert_eq!(DEFAULT_COLS, &["confidence"]);
        // The hand-written slices are what parse/serialize read; the table
        // is what the picker and the grid read. They must agree.
        let ids: Vec<&str> = RECIPE_COLUMNS.iter().filter(|c| !c.id.is_empty()).map(|c| c.id).collect();
        assert_eq!(ids, OPTIONAL_COLUMN_ORDER.to_vec());
        let defaults: Vec<&str> = RECIPE_COLUMNS.iter().filter(|c| !c.id.is_empty() && c.default_on).map(|c| c.id).collect();
        assert_eq!(defaults, DEFAULT_COLS.to_vec());
    }

    #[test]
    fn every_recipe_sort_mode_is_catalogued_exactly_once() {
        for mode in [
            SortMode::Roi, SortMode::Profit, SortMode::Velocity, SortMode::CostPerUnit,
            SortMode::Price, SortMode::AvgPrice, SortMode::LastSold, SortMode::Volume,
            SortMode::Vwap, SortMode::Tax, SortMode::Confidence,
        ] {
            let hits = RECIPE_COLUMNS
                .iter()
                .filter(|c| matches!(c.sort, Sortability::By(m) if m == mode))
                .count();
            assert_eq!(hits, 1, "{mode:?} catalogued {hits} times");
            assert_eq!(mode.to_string().parse::<SortMode>(), Ok(mode));
        }
        assert_eq!(SortMode::CostPerUnit.default_dir(), SortDir::Asc);
        assert_eq!(SortMode::Profit.default_dir(), SortDir::Desc);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros-app --lib recipe_optional_column_order`
Expected: FAIL to compile (`RECIPE_COLUMNS` missing).

- [ ] **Step 3: Add the specs, cell functions and the static table** (insert immediately after `const DEFAULT_COLS: &[&str] = &[COL_CONFIDENCE];`; the `COL_*` consts, `OPTIONAL_COLUMN_ORDER` and `DEFAULT_COLS` stay exactly as they are)

```rust
use crate::analyzer_kit::cells::CellValue;
use crate::analyzer_kit::columns::{
    CellCtx, ColumnKind, ColumnSpec, Layer, Sortability, ToolColumnMeta, default_dir_for,
    picker_options, sort_from_token, sort_token, sortability_for,
};
use crate::analyzer_kit::grid::{AnalyzerGrid, AnalyzerRow, GridLayout};

type RecipeRow = Arc<RecipeProfitData>;

impl AnalyzerRow for RecipeRow {
    type Key = xiv_gen::RecipeId;
    fn key(&self) -> Self::Key {
        self.recipe.key_id
    }
}

// Labels: one fn per column so the table can be a `static`.
fn label_item(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, item).to_string() }
fn label_profit(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, profit).to_string() }
fn label_roi(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, roi).to_string() }
fn label_cost(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, recipe_analyzer_col_cost_per_unit).to_string() }
fn label_price(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, price).to_string() }
fn label_daily(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, daily_sales).to_string() }
fn label_avg(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, avg_price).to_string() }
fn label_confidence(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, analyzer_col_confidence).to_string() }
fn label_last_sold(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, analyzer_col_last_sold).to_string() }
fn label_volume(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, recipe_analyzer_col_volume).to_string() }
fn label_vwap(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, recipe_analyzer_col_vwap).to_string() }
fn label_tax(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, analyzer_col_tax).to_string() }
fn label_world(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, analyzer_col_world).to_string() }
fn label_dc(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, analyzer_col_datacenter).to_string() }
fn label_actions(i18n: I18nContext<Locale, I18nKeys>) -> String { t_string!(i18n, actions).to_string() }

static SPEC_ITEM: ColumnSpec = ColumnSpec { kind: ColumnKind::Item, label: label_item };
static SPEC_PROFIT: ColumnSpec = ColumnSpec { kind: ColumnKind::Profit, label: label_profit };
static SPEC_ROI: ColumnSpec = ColumnSpec { kind: ColumnKind::Roi, label: label_roi };
static SPEC_COST: ColumnSpec = ColumnSpec { kind: ColumnKind::CostSlot, label: label_cost };
static SPEC_PRICE: ColumnSpec = ColumnSpec { kind: ColumnKind::RevenueSlot, label: label_price };
static SPEC_DAILY: ColumnSpec = ColumnSpec { kind: ColumnKind::SalesPerDay7, label: label_daily };
static SPEC_AVG: ColumnSpec = ColumnSpec { kind: ColumnKind::AvgPrice, label: label_avg };
static SPEC_CONFIDENCE: ColumnSpec = ColumnSpec { kind: ColumnKind::Confidence, label: label_confidence };
static SPEC_LAST_SOLD: ColumnSpec = ColumnSpec { kind: ColumnKind::LastSold, label: label_last_sold };
static SPEC_VOLUME: ColumnSpec = ColumnSpec { kind: ColumnKind::VolumeUnits7, label: label_volume };
static SPEC_VWAP: ColumnSpec = ColumnSpec { kind: ColumnKind::Vwap7, label: label_vwap };
static SPEC_TAX: ColumnSpec = ColumnSpec { kind: ColumnKind::Tax, label: label_tax };
static SPEC_WORLD: ColumnSpec = ColumnSpec { kind: ColumnKind::ListingWorld, label: label_world };
static SPEC_DC: ColumnSpec = ColumnSpec { kind: ColumnKind::ListingDc, label: label_dc };
static SPEC_ACTIONS: ColumnSpec = ColumnSpec { kind: ColumnKind::Actions, label: label_actions };

// Cell extractors. `Custom` = the page renders it (needs context the row
// does not carry: item names, the world link, the on-hand list button).
fn cell_custom(_: &RecipeRow, _: &CellCtx) -> CellValue { CellValue::Custom }
fn cell_profit(r: &RecipeRow, _: &CellCtx) -> CellValue { CellValue::Gil(r.profit) }
fn cell_roi(r: &RecipeRow, _: &CellCtx) -> CellValue { CellValue::RoiBadge(r.return_on_investment) }
fn cell_price(r: &RecipeRow, _: &CellCtx) -> CellValue { CellValue::Gil(r.market_price) }
fn cell_avg(r: &RecipeRow, _: &CellCtx) -> CellValue { CellValue::Gil(r.avg_price) }
fn cell_confidence(r: &RecipeRow, _: &CellCtx) -> CellValue { CellValue::Confidence(r.confidence) }
fn cell_last_sold(r: &RecipeRow, _: &CellCtx) -> CellValue { CellValue::LastSoldUnix(r.last_sold_unix) }
fn cell_volume(r: &RecipeRow, _: &CellCtx) -> CellValue { CellValue::Count(r.units_sold) }
fn cell_vwap(r: &RecipeRow, _: &CellCtx) -> CellValue { CellValue::GilWithPct { amount: r.vwap, pct: r.vwap_pct } }
fn cell_tax(r: &RecipeRow, _: &CellCtx) -> CellValue { CellValue::Gil(r.tax) }

const CELL_R: &str = "px-4 py-2 w-32 shrink-0 text-right";
const CELL_R_MD: &str = "px-4 py-2 w-32 shrink-0 text-right hidden md:block";
const CELL_28_MD: &str = "px-4 py-2 w-28 shrink-0 text-right hidden md:block";
const HEAD: &str = "w-32 shrink-0 p-4";
const HEAD_MD: &str = "w-32 shrink-0 p-4 hidden md:block";
const HEAD_28_MD: &str = "w-28 shrink-0 p-4 hidden md:block";

/// The recipe table, column by column, classes copied verbatim from the
/// markup this replaced. `id` = the `?cols=` token (always-on columns
/// have none); `sort_id` = the `?sort=` token.
static RECIPE_COLUMNS: [ToolColumnMeta<RecipeRow, SortMode>; 15] = [
    ToolColumnMeta { spec: &SPEC_ITEM, id: "", sort_id: "", sort: Sortability::No, default_dir: SortDir::Desc, header_class: "w-64 md:w-80 shrink-0 p-4", cell_class: "", default_on: true, cell: cell_custom },
    ToolColumnMeta { spec: &SPEC_PROFIT, id: "", sort_id: "profit", sort: sortability_for(Layer::Computed, Some(SortMode::Profit)), default_dir: SortDir::Desc, header_class: HEAD, cell_class: CELL_R, default_on: true, cell: cell_profit },
    ToolColumnMeta { spec: &SPEC_ROI, id: "", sort_id: "roi", sort: sortability_for(Layer::Computed, Some(SortMode::Roi)), default_dir: SortDir::Desc, header_class: HEAD, cell_class: CELL_R, default_on: true, cell: cell_roi },
    ToolColumnMeta { spec: &SPEC_COST, id: "", sort_id: "cost", sort: sortability_for(Layer::Computed, Some(SortMode::CostPerUnit)), default_dir: SortDir::Asc, header_class: HEAD, cell_class: "", default_on: true, cell: cell_custom },
    ToolColumnMeta { spec: &SPEC_PRICE, id: "", sort_id: "price", sort: sortability_for(Layer::RowLocal, Some(SortMode::Price)), default_dir: SortDir::Desc, header_class: HEAD, cell_class: CELL_R, default_on: true, cell: cell_price },
    ToolColumnMeta { spec: &SPEC_DAILY, id: "", sort_id: "velocity", sort: sortability_for(Layer::Bulk, Some(SortMode::Velocity)), default_dir: SortDir::Desc, header_class: HEAD_MD, cell_class: "", default_on: true, cell: cell_custom },
    ToolColumnMeta { spec: &SPEC_AVG, id: "", sort_id: "avg-price", sort: sortability_for(Layer::Bulk, Some(SortMode::AvgPrice)), default_dir: SortDir::Desc, header_class: HEAD_MD, cell_class: CELL_R_MD, default_on: true, cell: cell_avg },
    ToolColumnMeta { spec: &SPEC_CONFIDENCE, id: COL_CONFIDENCE, sort_id: "confidence", sort: sortability_for(Layer::Bulk, Some(SortMode::Confidence)), default_dir: SortDir::Desc, header_class: HEAD_28_MD, cell_class: "px-4 py-2 w-28 shrink-0 flex items-center justify-end hidden md:flex", default_on: true, cell: cell_confidence },
    ToolColumnMeta { spec: &SPEC_LAST_SOLD, id: COL_LAST_SOLD, sort_id: "last-sold", sort: sortability_for(Layer::Bulk, Some(SortMode::LastSold)), default_dir: SortDir::Desc, header_class: HEAD_28_MD, cell_class: CELL_28_MD, default_on: false, cell: cell_last_sold },
    ToolColumnMeta { spec: &SPEC_VOLUME, id: COL_VOLUME, sort_id: "volume", sort: sortability_for(Layer::Bulk, Some(SortMode::Volume)), default_dir: SortDir::Desc, header_class: HEAD_28_MD, cell_class: "px-4 py-2 w-28 shrink-0 text-right hidden md:block font-mono tabular-nums", default_on: false, cell: cell_volume },
    ToolColumnMeta { spec: &SPEC_VWAP, id: COL_VWAP, sort_id: "vwap", sort: sortability_for(Layer::Bulk, Some(SortMode::Vwap)), default_dir: SortDir::Desc, header_class: HEAD_MD, cell_class: CELL_R_MD, default_on: false, cell: cell_vwap },
    ToolColumnMeta { spec: &SPEC_TAX, id: COL_TAX, sort_id: "tax", sort: sortability_for(Layer::Computed, Some(SortMode::Tax)), default_dir: SortDir::Desc, header_class: HEAD_28_MD, cell_class: CELL_28_MD, default_on: false, cell: cell_tax },
    ToolColumnMeta { spec: &SPEC_WORLD, id: COL_LISTING_WORLD, sort_id: "", sort: Sortability::No, default_dir: SortDir::Desc, header_class: HEAD_28_MD, cell_class: "", default_on: false, cell: cell_custom },
    ToolColumnMeta { spec: &SPEC_DC, id: COL_LISTING_DC, sort_id: "", sort: Sortability::No, default_dir: SortDir::Desc, header_class: HEAD_28_MD, cell_class: "", default_on: false, cell: cell_custom },
    ToolColumnMeta { spec: &SPEC_ACTIONS, id: "", sort_id: "", sort: Sortability::No, default_dir: SortDir::Desc, header_class: "w-20 shrink-0 p-4", cell_class: "", default_on: true, cell: cell_custom },
];
```

The `SPEC_*` statics stay in the route for this phase; Phase G lifts the ones the flip finder shares into `columns.rs`.

- [ ] **Step 4: Delegate `SortMode`'s `FromStr`, `Display` and `default_dir` to the table**

Replace the three impls, `impl FromStr for SortMode` through the closing brace of `impl SortColumn for SortMode`, with:

```rust
impl FromStr for SortMode {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        sort_from_token(&RECIPE_COLUMNS, s).ok_or(())
    }
}

impl Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Every variant is catalogued exactly once (pinned by test); the
        // fallback token only guards against a future variant added to the
        // enum before the table.
        f.write_str(sort_token(&RECIPE_COLUMNS, *self).unwrap_or("profit"))
    }
}

impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Profit
    }
    fn default_dir(self) -> SortDir {
        default_dir_for(&RECIPE_COLUMNS, self)
    }
}
```

- [ ] **Step 5: Replace the picker plumbing** (from the comment `// Optional-column picker, flip-finder style.` through the `});` that closes `let column_options = Signal::derive(…)`)

```rust
    let column_options = Signal::derive(move || picker_options(&RECIPE_COLUMNS, i18n));
```

Delete `col_label`; `toggle_column` and `reset_columns` stay.

- [ ] **Step 6: Build the `custom` renderer and replace the VirtualScroller block**

Above the `view!` in `RecipeAnalyzerTable`, add the custom renderer. Every branch is the old cell's markup, verbatim, keyed by `spec.kind`:

```rust
    let world_names_for_cells = world_names.clone();
    let custom: Arc<dyn Fn(&RecipeRow, ColumnKind) -> AnyView + Send + Sync> = Arc::new(move |data, kind| {
        let data = data.clone();
        let item_id = ItemId(data.recipe.item_result);
        match kind {
            ColumnKind::Item => {
                let item = items.get(&item_id).map(|i| i.name.as_str()).unwrap_or("Unknown");
                let item_level = items.get(&item_id).map(|i| i.level_item).unwrap_or(0);
                let job_abbrev = craft_type_acronym(data.recipe.craft_type);
                view! {
                    <div role="cell" class="px-4 py-2 flex flex-row w-64 md:w-80 shrink-0 items-center gap-2">
                        <a
                            class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip w-full"
                            href=format!("/item/{}/{}", world(), item_id.0)
                        >
                            <div class="shrink-0">
                                <ItemIcon item_id=item_id.0 icon_size=IconSize::Small />
                            </div>
                            <div class="flex flex-col">
                                <span>{item}</span>
                                <span class="text-xs text-[color:var(--color-text-muted)]">
                                    "Lv " {data.required_level} " • iLv " {item_level} " " {job_abbrev}
                                </span>
                            </div>
                        </a>
                    </div>
                }
                .into_any()
            }
            ColumnKind::CostSlot => {
                // Paste the old Cost / unit cell (Gil + yield note + subcraft
                // Tooltip) verbatim from the pre-refactor markup, reading
                // `data` instead of the closure's `data`.
                todo!("paste the old Cost / unit cell here")
            }
            ColumnKind::SalesPerDay7 => {
                let sales_tooltip = format!(
                    "Based on {} sales over {:.1} days",
                    data.total_sales,
                    (data.total_sales as f32 / data.daily_sales.max(0.001))
                );
                view! {
                    <div role="cell" class="px-4 py-2 w-32 shrink-0 text-right hidden md:block">
                        <span class="text-xs text-[color:var(--color-text-muted)]" title=sales_tooltip>
                            {format!("{:.1} / day", data.daily_sales)}
                        </span>
                    </div>
                }
                .into_any()
            }
            ColumnKind::ListingWorld | ColumnKind::ListingDc => {
                // The old row computed `listing_location` once per row;
                // recompute it here, then paste the old listing World / DC
                // cells verbatim (the QueryButton + Tooltip + "—" fallback),
                // choosing the World arm or the DC arm by `kind`.
                let listing_location = world_names_for_cells.get(&data.cheapest_world_id).cloned();
                todo!("paste the old listing world/dc cells here")
            }
            ColumnKind::Actions => view! {
                <div role="cell" class="px-4 py-2 w-20 shrink-0">
                    <AddRecipeToList recipe=data.recipe />
                </div>
            }
            .into_any(),
            other => unreachable!("no custom cell for column {other:?}"),
        }
    });

    fn stripe(index: usize) -> &'static str {
        if index % 2 == 0 {
            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
        } else {
            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
        }
    }
    let cell_ctx = Signal::derive(|| CellCtx { now_unix: chrono::Utc::now().timestamp() });
```

The two `todo!` arms are filled by pasting the corresponding blocks from the current file (Cost: the `<div role="cell" class="px-4 py-2 w-32 shrink-0 text-right">` that wraps `<Gil amount=data.cost />`, through its closing `</div>`; listing World/DC: the two `{ let loc = listing_location.clone(); move || visible_cols.get().contains(COL_LISTING_WORLD | COL_LISTING_DC) … }` blocks, each through its closing `}`) — they are page-owned markup and must not change, apart from reading the `listing_location` recomputed above. Do that before compiling; a `todo!` is not allowed to remain.

Then replace the whole table block — the `<div class="rounded-2xl overflow-x-auto panel …">` wrapper around `<VirtualScroller … />`, through the `</div>` that follows the scroller's closing `/>` — with:

```rust
             <div class="rounded-2xl overflow-x-auto panel content-visible contain-layout contain-paint will-change-scroll forced-layer">
                <AnalyzerGrid
                    columns=&RECIPE_COLUMNS
                    rows=computed_data
                    visible_cols=visible_cols
                    sort_mode=sort_mode
                    sort_dir=sort_dir
                    ctx=cell_ctx
                    custom=custom
                    layout=GridLayout { viewport_height: 720.0, row_height: 60.0, header_height: 64.0, overscan: 8 }
                    header_class="flex flex-row align-top h-16 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]"
                    row_class=stripe
                />
             </div>
```

Delete the now-unused imports (`SortableHeaderCell`, `Gil` if unused, `ConfidenceBadge` if unused, `last_sold_label`) — clippy will list them.

- [ ] **Step 7: Run everything**

Run: `cargo test -p ultros-app --lib`
Expected: PASS, including `price_rows_matches_recorded_oracle_on_fixture` untouched, `sort_mode_round_trips_through_the_url` untouched, and the two new tests.

Run: `cargo fmt --all && cargo clippy -p ultros-app --all-targets -- -D warnings`
Expected: clean, no dead code anywhere in `analyzer_kit`.

- [ ] **Step 8: Manual parity**

Serve the branch and `main` side by side on `/recipe-analyzer?world=Gilgamesh`, then with `?cols=confidence,last-sold,volume,vwap,tax,listing-world,listing-dc` and with `?cols=`. Check the same columns in the same order and width at 1440 and at 375 px, the same rows, header sort arrows landing on the same column for `?sort=cost`, the picker listing the same seven labels in the same order, and the VWAP column reading identically for rows with a VWAP (rows without one show the dash at the left of the cell on this branch and at the right on `main` — expected, see Global Constraints). Record `pkg/*.wasm` size before and after in the PR description (`ls -la target/site/pkg/*.wasm` after `cargo leptos build --release`).

- [ ] **Step 9: Commit and open the PR**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "refactor(recipe-analyzer): render the table from a static column table via AnalyzerGrid"
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git push -u origin HEAD
gh pr create --base main --title "Analyzer kit phase B: column kit and recipe table adoption" --body "Part of #1233. Same pixels, same numbers, same URL contract; the recipe table is now rendered from a static column table through the shared AnalyzerGrid. See docs/superpowers/plans/2026-09-01-analyzer-kit-phase-b-column-kit.md.

- analyzer_kit::{columns, cells, grid}
- recipe analyzer: RECIPE_COLUMNS static drives ?cols=, the picker and every ?sort= token; ~250 fewer lines of hand-written header/cell markup
- VWAP cell keeps one element shape (was an arm switch); the no-VWAP dash moves from right- to left-aligned, matching the Gil amounts
- wasm size before/after: <fill in>

Tests: cargo test -p ultros-app --lib green (oracle untouched), ./check_ci.sh clean; manual parity vs main at 1440 and 375 px."
```

---

## Self-review

**Spec coverage (kit spec Phase B):** `columns`/`cells`/`grid` modules → Tasks 1-3; `SortableHeaderCell` optional props and `ColumnOption.group/disabled/hint` → deferred to Phase C (their first consumers); `parse/serialize_visible_cols` signature relaxation → not needed (the page keeps `&'static` slices pinned equal to the table); `--tool-fixed-cols` → Phase G; recipe adoption with `RECIPE_COLUMNS`, `impl AnalyzerRow`, custom cells, `SortMode` delegation, header/cell blocks deleted, VWAP arm switch fixed → Task 4; `visible_range` wiring → Phase E2 (no consumer here); header label reactivity → sortable labels stay one-shot `t_string!` as today, unsortable ones stay locale-reactive through a closure as their `t!` was; `ColumnSpec.canonical_id` and the `CATALOG` array → Phase G (no reader here); currency-exchange cols parser deletion, `AnalyzerGridSkeleton` and header hscroll sync → deferred (no consumer in the recipe table this phase; the recipe panel keeps its `overflow-x-auto` wrapper).

**Placeholder scan:** the two `todo!` arms in Task 4 Step 6 are explicit paste instructions with line ranges and must be resolved before the build step in the same task.

**Type consistency:** `ToolColumnMeta<RecipeRow, SortMode>` with `cell: fn(&RecipeRow, &CellCtx) -> CellValue` matches the extractor signatures; `custom` is `Arc<dyn Fn(&RecipeRow, ColumnKind) -> AnyView + Send + Sync>` and the grid calls it with `col.spec.kind`; `computed_data: Memo<Vec<(usize, Arc<RecipeProfitData>)>>` from Phase A feeds `rows`; `sort_mode`/`sort_dir` are the page-level memos passed as props in Phase A.
