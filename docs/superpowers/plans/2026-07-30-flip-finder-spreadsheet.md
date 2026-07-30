# Flip Finder Spreadsheet Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the flip finder into a contained spreadsheet pane with resizable/removable columns, a header context menu, clearer sell-world copy, and Realistic-flips default filters.

**Architecture:** The table moves from window-scroll virtualization (two JS-mirrored horizontal scrollports) to `ScrollSource::Container` with a new `fill` mode on `VirtualScroller`: one scrollport for both axes, header sticky inside it. Column widths collapse into a single data-driven registry rendered as CSS custom properties, with drag-resize persisted to localStorage. Defaults seed into the URL via the existing query-seeding mechanism.

**Tech Stack:** Rust, Leptos 0.8 (nightly feature, SSR+hydration), leptos-use 0.18, leptos-i18n 0.6 (7 locales), Tailwind CSS 4.

**Spec:** `docs/superpowers/specs/2026-07-30-flip-finder-spreadsheet-design.md`

**Key files:**
- `ultros-frontend/ultros-app/src/routes/analyzer.rs` — the whole flip finder (3128 lines)
- `ultros-frontend/ultros-app/src/routes/analyzer_columns.rs` — NEW: column registry
- `ultros-frontend/ultros-app/src/components/virtual_scroller.rs` — gets `fill` mode
- `ultros-frontend/ultros-app/src/query_defaults.rs` — gets multi-param seeding
- `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` — copy + new keys
- `style/tailwind.css` lines ~1961–2106 — analyzer CSS

**Conventions that apply to every task:**
- Run tests with `cargo test -p ultros-app` (add `<test_name>` to filter).
- Before every commit: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log` — the echo is required; piping into tail directly reports the pipe's exit code, not the script's. `cargo fmt --all` autofixes formatting failures. Exit 137 = clippy OOM-killed, not a lint failure; re-run `cargo clippy --all-targets -j 2 -- -D warnings`.
- Commit messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Every user-facing string goes through `t!(i18n, key)` / `t_string!(i18n, key)` and needs the key in ALL 7 locale files with real translations.
- The `view!` macro is Leptos, not JSX: `on:click=move |_| …`, `prop:checked=…`, `class:hidden=move || …`.

---

### Task 0: Initialize submodules (build prerequisite)

The worktree has no submodules checked out (`xiv-gen/ffxiv-datamining/csv/en/Item.csv` is missing), and `cargo test`/clippy compile `xiv-gen-db`, whose build script panics without them. Do NOT use `git submodule update --init --recursive` — it fails three different ways here (see CLAUDE.md). Use the `--reference` recipe against the main clone.

- [ ] **Step 0.1: Init each submodule against the main clone**

```bash
cd /Users/aaronkarras/code/ffxiv-playground/.claude/worktrees/flip-finder-spreadsheet-redesign-dff784
MAIN=/Users/aaronkarras/code/ffxiv-playground

git submodule update --init --reference $MAIN/.git/modules/ultros-frontend/universalis-assets ultros-frontend/ultros-xiv-icons/universalis-assets
git submodule update --init --reference $MAIN/.git/modules/xiv-gen/ffxiv-datamining xiv-gen/ffxiv-datamining
git submodule update --init --force ultros/static/classjob-icons

M=$MAIN/.git/modules/xiv-gen/ffxiv-datamining/modules/csv
for s in cn ko tc; do
  git -C xiv-gen/ffxiv-datamining submodule update --init --reference "$M/$s" "csv/$s"
done
```

- [ ] **Step 0.2: Verify (do not trust exit codes)**

```bash
ls xiv-gen/ffxiv-datamining/csv/{en,cn,tc}/Item.csv xiv-gen/ffxiv-datamining/csv/ko/csv/Item.csv
ls ultros-frontend/ultros-xiv-icons/universalis-assets/icon2x | head -1
ls ultros/static/classjob-icons | wc -l   # must be non-zero
git status --short                        # no submodule may show as modified
```

Expected: all four `Item.csv` paths exist (`csv/ko/csv/` nesting is correct), icon2x non-empty, classjob-icons non-zero, clean status. If a submodule ends up empty/broken, remove its per-worktree gitdir under `.git/modules` before retrying.

- [ ] **Step 0.3: Baseline check** — `cargo test -p ultros-app > /tmp/base.log 2>&1; echo "REAL_EXIT=$?"; tail -5 /tmp/base.log`. Expected: `REAL_EXIT=0`. No commit for this task.

---

### Task 1: Column registry module

One source of truth for column ids, widths, and `?cols=` visibility. Moves the existing `COL_*` constants + `parse_visible_cols`/`serialize_visible_cols` out of `analyzer.rs` and adds width specs. `extra_column_width_px` stays in `analyzer.rs` for now (deleted in Task 6).

**Files:**
- Create: `ultros-frontend/ultros-app/src/routes/analyzer_columns.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/mod.rs` (line 3 area)
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (delete lines 66–133, add import)

- [ ] **Step 1.1: Write the new module with failing-to-compile references first? No — this is a move + new pure functions; write tests inside the new module.** Create `ultros-frontend/ultros-app/src/routes/analyzer_columns.rs`:

```rust
//! Column registry for the Flip Finder table.
//!
//! One entry per rendered column, in DOM order. This is the single source
//! of truth for column ids, default/minimum widths, resizability, and
//! `?cols=` visibility — replacing widths that used to be encoded three
//! times (Tailwind class on the header cell, again on the row cell, and a
//! px table in `extra_column_width_px`).
//!
//! Widths render as CSS custom properties (`--colw-<id>`) on the table
//! pane; header and row cells both size themselves with
//! `width: var(--colw-<id>)`, and the row min-width is a `calc()` sum of
//! the visible columns' variables so a live drag updates everything in one
//! style write.

use std::collections::{HashMap, HashSet};

// Required columns — always render, not present in `?cols=`.
pub const COL_HQ: &str = "hq";
pub const COL_ITEM: &str = "item";
pub const COL_PROFIT: &str = "profit";
pub const COL_BUY_PRICE: &str = "buy_price";

/// Stable URL IDs for optional columns. Order here is the columns-picker +
/// `?cols=` serialization order: default-on columns first, opt-ins after.
/// It is deliberately *not* the DOM order — [`COLUMNS`] is DOM order.
pub const COL_PROFIT_PER_DAY: &str = "profit_per_day";
pub const COL_VELOCITY: &str = "velocity";
pub const COL_DRIFT: &str = "drift";
pub const COL_CONFIDENCE: &str = "confidence";
pub const COL_WORLD: &str = "world";
pub const COL_LAST_SOLD: &str = "last_sold";
pub const COL_ROI: &str = "roi";
pub const COL_DATACENTER: &str = "datacenter";
pub const COL_TREND: &str = "trend";
pub const COL_SALES_PER_DAY: &str = "sales_per_day";
pub const COL_VOLUME_30D: &str = "volume_30d";

pub const ALL_OPTIONAL_COLS: &[&str] = &[
    COL_PROFIT_PER_DAY,
    COL_VELOCITY,
    COL_DRIFT,
    COL_CONFIDENCE,
    COL_WORLD,
    COL_LAST_SOLD,
    COL_ROI,
    COL_DATACENTER,
    COL_TREND,
    COL_SALES_PER_DAY,
    COL_VOLUME_30D,
];

/// Default visible set when `?cols=` is absent from the URL. Once the
/// user explicitly sets the param (even to ""), we respect that exact
/// set instead of falling back to defaults.
///
/// ClickHouse-only columns (trend, sales/day, 30d volume) are off because
/// the rollup covers ~7% of traded items, so they would be blank on most
/// rows. ROI is off because it ranks by ratio, which is the wrong
/// objective when retainer slots are the scarce resource.
pub const DEFAULT_VISIBLE_COLS: &[&str] = &[
    COL_PROFIT_PER_DAY,
    COL_VELOCITY,
    COL_DRIFT,
    COL_CONFIDENCE,
    COL_WORLD,
    COL_LAST_SOLD,
];

/// localStorage key for user width overrides (`HashMap<String, f64>`, px).
pub const COL_WIDTHS_KEY: &str = "ultros.flipfinder.colwidths";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnSpec {
    pub id: &'static str,
    /// Rendered width in px when the user has not resized the column.
    /// Values carried over from the Tailwind classes they replace
    /// (`w-28` = 112px, `w-[88px]` = 88px, …).
    pub default_width: f64,
    /// Drag-resize floor. Keeps every column wide enough to remain
    /// clickable/readable; also the clamp for stored overrides.
    pub min_width: f64,
    pub resizable: bool,
    /// Participates in `?cols=` visibility. Required columns are `false`.
    pub optional: bool,
}

/// Every column in DOM order. The markup in analyzer.rs renders exactly
/// this sequence.
pub const COLUMNS: &[ColumnSpec] = &[
    ColumnSpec { id: COL_HQ, default_width: 44.0, min_width: 44.0, resizable: false, optional: false },
    ColumnSpec { id: COL_ITEM, default_width: 288.0, min_width: 140.0, resizable: true, optional: false },
    ColumnSpec { id: COL_PROFIT, default_width: 112.0, min_width: 90.0, resizable: true, optional: false },
    ColumnSpec { id: COL_PROFIT_PER_DAY, default_width: 112.0, min_width: 90.0, resizable: true, optional: true },
    ColumnSpec { id: COL_VELOCITY, default_width: 88.0, min_width: 70.0, resizable: true, optional: true },
    ColumnSpec { id: COL_DRIFT, default_width: 88.0, min_width: 70.0, resizable: true, optional: true },
    ColumnSpec { id: COL_CONFIDENCE, default_width: 72.0, min_width: 60.0, resizable: true, optional: true },
    ColumnSpec { id: COL_ROI, default_width: 112.0, min_width: 80.0, resizable: true, optional: true },
    ColumnSpec { id: COL_BUY_PRICE, default_width: 112.0, min_width: 90.0, resizable: true, optional: false },
    ColumnSpec { id: COL_WORLD, default_width: 112.0, min_width: 80.0, resizable: true, optional: true },
    ColumnSpec { id: COL_DATACENTER, default_width: 112.0, min_width: 80.0, resizable: true, optional: true },
    ColumnSpec { id: COL_TREND, default_width: 100.0, min_width: 80.0, resizable: true, optional: true },
    ColumnSpec { id: COL_SALES_PER_DAY, default_width: 140.0, min_width: 90.0, resizable: true, optional: true },
    ColumnSpec { id: COL_VOLUME_30D, default_width: 88.0, min_width: 70.0, resizable: true, optional: true },
    ColumnSpec { id: COL_LAST_SOLD, default_width: 112.0, min_width: 80.0, resizable: true, optional: true },
];

pub fn column_spec(id: &str) -> Option<&'static ColumnSpec> {
    COLUMNS.iter().find(|c| c.id == id)
}

/// Effective width of a column: the stored override clamped to the spec's
/// minimum, or the default. Unknown override ids are simply ignored by the
/// callers (they iterate [`COLUMNS`], never the map).
pub fn effective_width(spec: &ColumnSpec, overrides: &HashMap<String, f64>) -> f64 {
    overrides
        .get(spec.id)
        .copied()
        .map(|w| w.max(spec.min_width))
        .unwrap_or(spec.default_width)
}

/// Whether a column currently renders: required columns always, optional
/// columns per the `?cols=` set.
pub fn is_column_visible(spec: &ColumnSpec, visible: &HashSet<&'static str>) -> bool {
    !spec.optional || visible.contains(spec.id)
}

/// Inline `style` for the table pane: one `--colw-<id>` per column plus
/// `--analyzer-row-min-width` as a `calc()` sum of the *visible* columns.
/// Using a calc-of-vars (rather than a precomputed px sum) means a live
/// drag that rewrites a single `--colw-*` property updates the row width
/// for free, with no Rust in the loop.
pub fn colw_style(
    visible: &HashSet<&'static str>,
    overrides: &HashMap<String, f64>,
) -> String {
    let mut style = String::new();
    let mut sum_terms: Vec<String> = Vec::new();
    for spec in COLUMNS {
        let width = effective_width(spec, overrides);
        style.push_str(&format!("--colw-{}:{}px;", spec.id, width));
        if is_column_visible(spec, visible) {
            sum_terms.push(format!("var(--colw-{})", spec.id));
        }
    }
    style.push_str(&format!(
        "--analyzer-row-min-width:calc({});",
        sum_terms.join(" + ")
    ));
    style
}

pub fn parse_visible_cols(raw: Option<&str>) -> HashSet<&'static str> {
    match raw {
        None => DEFAULT_VISIBLE_COLS.iter().copied().collect(),
        Some(s) => s
            .split(',')
            .filter_map(|tok| ALL_OPTIONAL_COLS.iter().find(|c| **c == tok).copied())
            .collect(),
    }
}

pub fn serialize_visible_cols(visible: &HashSet<&'static str>) -> String {
    ALL_OPTIONAL_COLS
        .iter()
        .filter(|c| visible.contains(*c))
        .copied()
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_optional_col_has_a_spec_and_vice_versa() {
        for id in ALL_OPTIONAL_COLS {
            let spec = column_spec(id).expect("optional col must be in COLUMNS");
            assert!(spec.optional, "{id} spec must be marked optional");
        }
        let optional_in_columns: Vec<_> =
            COLUMNS.iter().filter(|c| c.optional).map(|c| c.id).collect();
        assert_eq!(optional_in_columns.len(), ALL_OPTIONAL_COLS.len());
    }

    #[test]
    fn effective_width_clamps_to_min_and_falls_back_to_default() {
        let spec = column_spec(COL_ITEM).unwrap();
        let mut overrides = HashMap::new();
        assert_eq!(effective_width(spec, &overrides), spec.default_width);
        overrides.insert(COL_ITEM.to_string(), 10.0);
        assert_eq!(effective_width(spec, &overrides), spec.min_width);
        overrides.insert(COL_ITEM.to_string(), 400.0);
        assert_eq!(effective_width(spec, &overrides), 400.0);
    }

    #[test]
    fn colw_style_sums_only_visible_columns() {
        let visible: HashSet<&'static str> = [COL_PROFIT_PER_DAY].into_iter().collect();
        let style = colw_style(&visible, &HashMap::new());
        // Required columns always in the sum:
        assert!(style.contains("var(--colw-hq)"));
        assert!(style.contains("var(--colw-item)"));
        assert!(style.contains("var(--colw-profit)"));
        assert!(style.contains("var(--colw-buy_price)"));
        assert!(style.contains("var(--colw-profit_per_day)"));
        // Hidden optional column: variable declared, but not in the sum.
        assert!(style.contains("--colw-roi:112px;"));
        let sum = style.split("--analyzer-row-min-width").nth(1).unwrap();
        assert!(!sum.contains("--colw-roi"));
    }

    #[test]
    fn default_widths_are_at_least_min_widths() {
        for spec in COLUMNS {
            assert!(
                spec.default_width >= spec.min_width,
                "{}: default {} < min {}",
                spec.id,
                spec.default_width,
                spec.min_width
            );
        }
    }
}
```

- [ ] **Step 1.2: Register the module.** In `ultros-frontend/ultros-app/src/routes/mod.rs`, next to `pub mod analyzer;` add:

```rust
pub mod analyzer_columns;
```

- [ ] **Step 1.3: Run the new tests** — `cargo test -p ultros-app analyzer_columns`. Expected: 4 passed.

- [ ] **Step 1.4: Switch analyzer.rs to the module.** In `ultros-frontend/ultros-app/src/routes/analyzer.rs`:
  - Delete lines 66–133 (the `COL_*` consts, `ALL_OPTIONAL_COLS`, `DEFAULT_VISIBLE_COLS`, `parse_visible_cols`, `serialize_visible_cols` — everything between the `is_settled` fn's closing brace and `use chrono::…`).
  - Add with the other `use` statements: `use super::analyzer_columns::*;`
  - The existing `#[cfg(test)] mod tests` uses `use super::*;` so any tests referencing the moved items keep compiling via the glob re-import.

- [ ] **Step 1.5: Full test run** — `cargo test -p ultros-app`. Expected: PASS (same count as baseline + 4).

- [ ] **Step 1.6: CI check + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add -A ultros-frontend/ultros-app/src/routes/
git commit -m "refactor(flip-finder): extract column registry into analyzer_columns

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: `fill` mode on VirtualScroller

Container mode currently needs a fixed `viewport_height` and lets its inner list div become a second (nested, header-desyncing) horizontal scrollport. `fill: true` makes the scroller (a) size itself to its parent (`height: 100%`) and measure its real height for the row math, and (b) act as the **single** scrollport for both axes by letting the list content's width propagate.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/virtual_scroller.rs`

- [ ] **Step 2.1: Add the prop.** In the `#[component] pub fn VirtualScroller` signature (after `row_min_width`, line ~164), add:

```rust
    /// Container-mode only: size the scroller to its parent (`height: 100%`)
    /// instead of `viewport_height`, measure the element's real height for
    /// the visible-row math (`viewport_height` remains the pre-measurement
    /// fallback), and make this element the single scrollport for BOTH axes
    /// — the row area keeps `overflow: visible` so its width propagates and
    /// the sticky header scrolls horizontally in lockstep with the rows.
    /// Ignored under [`ScrollSource::Window`].
    #[prop(optional)]
    fill: bool,
```

- [ ] **Step 2.2: Measure own height.** After the `let list: NodeRef<…>` binding (line ~226–229), add:

```rust
    // `fill` mode: the element's real height drives the row math. Signal is
    // 0.0 until the ResizeObserver first fires (and always on the server),
    // in which case `effective_viewport` below falls back to
    // `viewport_height`.
    let fill_height = RwSignal::new(0.0f64);
    if fill && !is_window {
        let size = leptos_use::use_element_size(scroller);
        Effect::new(move |_| fill_height.set(size.height.get()));
    }
```

- [ ] **Step 2.3: Use it in the viewport memo.** Replace the `effective_viewport` memo (line ~371–379):

```rust
    let effective_viewport = Memo::new(move |_| {
        if fill && !is_window {
            let h = fill_height.get() - header_h;
            if h > 0.0 {
                return h;
            }
        }
        viewport_px(
            source,
            window_height.get(),
            hydrated.get(),
            row_height,
            header_h,
        )
    });
```

- [ ] **Step 2.4: Container height + single scrollport.** Replace the `container_style` binding (line ~498–502):

```rust
    let container_style = if is_window {
        String::new()
    } else if fill {
        "height: 100%;".to_string()
    } else {
        format!("height: {}px;", viewport_height.ceil() as u32)
    };
```

Replace the list div's class/style (the `<div node_ref=list class="overflow-y-hidden …"` at line ~576–589). The current classes force the list div to be its own horizontal scrollport (`overflow-y: hidden` computes the x-axis to `auto`) and `contain-layout` blocks width propagation — both are exactly what `fill` must NOT do:

```rust
            <div
                node_ref=list
                class=if fill {
                    "will-change-[transform] relative w-full"
                } else {
                    "overflow-y-hidden overflow-x-visible will-change-[transform] relative w-full contain-layout forced-layer"
                }
                style=move || {
                    let min_width = if fill {
                        row_min_width
                            .as_ref()
                            .map(|w| format!("min-width: {w};"))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    format!(
                        r#"height: {}px;{min_width}"#,
                        {
                            let base = each.with(|children| children.len() as f64) * row_height;
                            let delta_total = fenwick.with(|f| f.sum(children_len()));
                            let bottom_pad = 16.0;
                            (base + delta_total + bottom_pad).ceil() as u32
                        },
                    )
                }>
```

(`row_min_width` is moved into the closure; the inner translateY div below already applies it too — keep that as is, both are needed so the spacer and the rendered slice agree.)

NOTE: `row_min_width` is an `Option<String>` moved into two closures now — clone it before the first: `let row_min_width_outer = row_min_width.clone();` and use one per closure.

- [ ] **Step 2.5: Header wrapper width in fill mode.** In the header render match (line ~548–564), the Container arm becomes:

```rust
                    ScrollSource::Container { .. } => {
                        // `w-max min-w-full` under `fill`: the wrapper must be
                        // as wide as the (overflowing) rows so the header
                        // scrolls horizontally with them instead of clipping
                        // at the pane width.
                        let class = if fill { "sticky top-0 z-10 w-max min-w-full" } else { "sticky top-0 z-10" };
                        view! { <div class=class>{h}</div> }.into_any()
                    }
```

- [ ] **Step 2.6: Compile + existing tests** — `cargo test -p ultros-app virtual_scroller`. Expected: existing `viewport_px`/Fenwick tests still pass (the new mode is opt-in; no call site passes `fill` yet).

- [ ] **Step 2.7: CI check + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add ultros-frontend/ultros-app/src/components/virtual_scroller.rs
git commit -m "feat(virtual-scroller): fill mode — parent-sized, single two-axis scrollport

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Realistic-flips default seeding

Seed the Realistic preset (plus `next-sale=1d`) into the URL when it arrives with no filter/sort params. Replaces the standalone `next-sale` seed on this route — a deliberate behavior change: a URL that already carries explicit filters no longer gets `next-sale=1d` silently appended, it renders exactly what it says.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/query_defaults.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (consts + the seed call at line ~2242)

- [ ] **Step 3.1: Write failing tests.** In `ultros-frontend/ultros-app/src/routes/analyzer.rs`'s `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn realistic_defaults_match_the_realistic_preset_plus_next_sale() {
        // The seeded set must stay in lockstep with the "Realistic flips"
        // built-in view (saved_views.rs) — same values, plus next-sale.
        let params: std::collections::HashMap<&str, &str> =
            REALISTIC_DEFAULT_PARAMS.iter().copied().collect();
        assert_eq!(params.get("min-buy"), Some(&"5000"));
        assert_eq!(params.get("last-sold"), Some(&"1d"));
        assert_eq!(params.get("roi"), Some(&"30"));
        assert_eq!(params.get("sort"), Some(&"profit-per-day"));
        assert_eq!(params.get("next-sale"), Some(&"1d"));
        assert_eq!(params.len(), 5);
        // The humantime values must actually parse, or the filter silently
        // becomes a no-op.
        assert!(humantime::parse_duration("1d").is_ok());
    }

    #[test]
    fn seeding_is_idempotent_because_every_seeded_key_suppresses_seeding() {
        for (key, _) in REALISTIC_DEFAULT_PARAMS {
            assert!(
                SEED_SUPPRESSING_PARAMS.contains(key),
                "seeded key {key} must also suppress seeding, or a reload loops"
            );
        }
    }

    #[test]
    fn suppression_covers_every_filter_but_not_view_config() {
        // Every addable filter + the chip-only filters + sort/dir suppress.
        for id in ADDABLE_FILTERS {
            assert!(SEED_SUPPRESSING_PARAMS.contains(id), "{id} must suppress");
        }
        for id in [FILTER_CATEGORY, FILTER_WORLD, FILTER_DATACENTER, "sort", "dir"] {
            assert!(SEED_SUPPRESSING_PARAMS.contains(&id), "{id} must suppress");
        }
        // View configuration is NOT a filter: a ?cols= bookmark or a region
        // toggle must still receive the default filters.
        for id in ["cols", "cross", "filter-outliers", "Europe", "Japan"] {
            assert!(!SEED_SUPPRESSING_PARAMS.contains(&id), "{id} must NOT suppress");
        }
    }
```

- [ ] **Step 3.2: Run to verify they fail** — `cargo test -p ultros-app suppression_covers`. Expected: FAIL, `SEED_SUPPRESSING_PARAMS` not found.

- [ ] **Step 3.3: Add the consts.** In `analyzer.rs`, directly below `ADDABLE_FILTERS` (line ~535):

```rust
/// Params whose presence means the visitor already chose filters, so the
/// Realistic-flips default must not be seeded on top. Everything in
/// [`ADDABLE_FILTERS`], the chip-only filters, and the sort params. View
/// configuration (`cols`, `cross`, `filter-outliers`, per-region toggles)
/// deliberately does not suppress: a columns bookmark still deserves the
/// default filters.
pub(crate) const SEED_SUPPRESSING_PARAMS: &[&str] = &[
    FILTER_PROFIT,
    FILTER_PROFIT_PER_DAY,
    FILTER_ROI,
    FILTER_SALES,
    FILTER_VELOCITY,
    FILTER_MIN_BUY,
    FILTER_MAX_PRICE,
    FILTER_NEXT_SALE,
    FILTER_LAST_SOLD,
    FILTER_PRE_TAX,
    FILTER_SHOW_SUSPICIOUS,
    FILTER_CATEGORY,
    FILTER_WORLD,
    FILTER_DATACENTER,
    "sort",
    "dir",
];

/// The "Realistic flips" built-in view's params (saved_views.rs) plus the
/// long-standing `next-sale=1d` velocity default — what a first-time
/// visitor lands on, rendered as removable chips.
pub(crate) const REALISTIC_DEFAULT_PARAMS: &[(&str, &str)] = &[
    ("min-buy", "5000"),
    ("last-sold", "1d"),
    ("roi", "30"),
    ("sort", "profit-per-day"),
    ("next-sale", DEFAULT_MAX_SALE_TIME),
];
```

- [ ] **Step 3.4: Run the three tests** — `cargo test -p ultros-app -- realistic_defaults seeding_is_idempotent suppression_covers`. Expected: 3 passed.

- [ ] **Step 3.5: Add the multi-param seeder.** In `query_defaults.rs`, after `seed_query_default`:

```rust
/// Seed a whole set of defaults in one navigation, but only when the URL
/// carries none of `suppressing_keys`.
///
/// One navigation rather than one [`seed_query_default`] per key: separate
/// seeds are separate effects, and an earlier one changing the URL makes a
/// later one's "is my key absent?" check race against router state — with a
/// presence *predicate* (not just per-key absence) that race would corrupt
/// the outcome, not just reorder it.
///
/// Same rule as [`seed_query_default`]: call from the **route** component,
/// never from inside a `Suspense` closure.
pub fn seed_query_defaults_when_unfiltered(
    suppressing_keys: &'static [&'static str],
    defaults: &'static [(&'static str, &'static str)],
) {
    let query = leptos_router::hooks::use_query_map();
    let location = leptos_router::hooks::use_location();
    let navigate = leptos_router::hooks::use_navigate();
    Effect::new(move |_| {
        let mut map = query.get_untracked();
        if suppressing_keys.iter().any(|k| map.get_str(k).is_some()) {
            return;
        }
        for (key, value) in defaults {
            map.insert(key.to_string(), value.to_string());
        }
        let path = location.pathname.get_untracked();
        navigate(
            &format!("{path}{}", map.to_query_string()),
            filter_nav_options(),
        );
    });
}
```

- [ ] **Step 3.6: Wire it into the route.** In `analyzer.rs`, `AnalyzerWorldView` (line ~2239–2242), replace:

```rust
    // Seeded here rather than in AnalyzerTable: that lives inside the Suspense
    // closure and remounts on every market refetch, which would keep undoing a
    // filter the user had cleared.
    seed_query_default("next-sale", DEFAULT_MAX_SALE_TIME.to_string());
```

with:

```rust
    // Seeded here rather than in AnalyzerTable: that lives inside the Suspense
    // closure and remounts on every market refetch, which would keep undoing a
    // filter the user had cleared. A URL with no filter/sort params at all
    // gets the Realistic-flips defaults (as removable chips); a URL carrying
    // any explicit filter is honored verbatim — including no longer getting
    // `next-sale=1d` silently appended.
    seed_query_defaults_when_unfiltered(SEED_SUPPRESSING_PARAMS, REALISTIC_DEFAULT_PARAMS);
```

Update the import from `crate::query_defaults` accordingly (add `seed_query_defaults_when_unfiltered`; `seed_query_default` may now be unused in this file — remove it from the import if so; `DEFAULT_MAX_SALE_TIME` is still used by `REALISTIC_DEFAULT_PARAMS`). Also update the stale comment at line ~751–753 (above the `next-sale` `filter_query_signal`) to say the default is seeded by `AnalyzerWorldView` as part of the Realistic defaults.

- [ ] **Step 3.7: Full test run** — `cargo test -p ultros-app`. Expected: PASS.

- [ ] **Step 3.8: CI check + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add ultros-frontend/ultros-app/src/query_defaults.rs ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "feat(flip-finder): seed Realistic-flips defaults for unfiltered visits

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: i18n — sell-world copy + context-menu keys

Two changed keys, six new keys, in ALL seven locale files. The `leptos_i18n` build fails if any locale is missing a key.

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/en.json` (existing keys at lines 430, 436; add new keys next to `analyzer_columns_picker_reset` ~line 1229)
- Modify: same keys/positions in `fr.json`, `de.json`, `ja.json`, `cn.json`, `ko.json`, `tc.json`

- [ ] **Step 4.1: Change the two copy keys in every locale.** Exact values per locale (`analyzer_select_world`, then `analyzer_index_choose_world`):

| locale | analyzer_select_world | analyzer_index_choose_world |
|---|---|---|
| en | `Sell on world:` | `Choose the world you'll sell your flips on:` |
| fr | `Monde de vente :` | `Choisissez le monde sur lequel vous revendrez vos objets :` |
| de | `Verkaufswelt:` | `Wähle die Welt, auf der du deine Flips verkaufen willst:` |
| ja | `販売先ワールド：` | `フリップ品を販売するワールドを選択してください：` |
| cn | `出售服务器：` | `选择你要出售商品的服务器：` |
| ko | `판매 서버:` | `아이템을 판매할 서버를 선택하세요:` |
| tc | `出售伺服器：` | `選擇你要出售商品的伺服器：` |

Find each key by grepping the locale file; the values live on one JSON line each.

- [ ] **Step 4.2: Add the six context-menu keys to every locale**, adjacent to `analyzer_columns_picker_reset` (keep the `analyzer_` grouping):

en:
```json
    "analyzer_menu_sort_asc": "Sort ascending",
    "analyzer_menu_sort_desc": "Sort descending",
    "analyzer_menu_hide_column": "Hide column",
    "analyzer_menu_reset_width": "Reset column width",
    "analyzer_menu_reset_all_widths": "Reset all column widths",
    "analyzer_menu_manage_columns": "Manage columns…",
```
fr:
```json
    "analyzer_menu_sort_asc": "Tri croissant",
    "analyzer_menu_sort_desc": "Tri décroissant",
    "analyzer_menu_hide_column": "Masquer la colonne",
    "analyzer_menu_reset_width": "Réinitialiser la largeur de la colonne",
    "analyzer_menu_reset_all_widths": "Réinitialiser toutes les largeurs",
    "analyzer_menu_manage_columns": "Gérer les colonnes…",
```
de:
```json
    "analyzer_menu_sort_asc": "Aufsteigend sortieren",
    "analyzer_menu_sort_desc": "Absteigend sortieren",
    "analyzer_menu_hide_column": "Spalte ausblenden",
    "analyzer_menu_reset_width": "Spaltenbreite zurücksetzen",
    "analyzer_menu_reset_all_widths": "Alle Spaltenbreiten zurücksetzen",
    "analyzer_menu_manage_columns": "Spalten verwalten …",
```
ja:
```json
    "analyzer_menu_sort_asc": "昇順で並べ替え",
    "analyzer_menu_sort_desc": "降順で並べ替え",
    "analyzer_menu_hide_column": "列を非表示",
    "analyzer_menu_reset_width": "列の幅をリセット",
    "analyzer_menu_reset_all_widths": "すべての列幅をリセット",
    "analyzer_menu_manage_columns": "列の管理…",
```
cn:
```json
    "analyzer_menu_sort_asc": "升序排序",
    "analyzer_menu_sort_desc": "降序排序",
    "analyzer_menu_hide_column": "隐藏列",
    "analyzer_menu_reset_width": "重置列宽",
    "analyzer_menu_reset_all_widths": "重置所有列宽",
    "analyzer_menu_manage_columns": "管理列…",
```
ko:
```json
    "analyzer_menu_sort_asc": "오름차순 정렬",
    "analyzer_menu_sort_desc": "내림차순 정렬",
    "analyzer_menu_hide_column": "열 숨기기",
    "analyzer_menu_reset_width": "열 너비 초기화",
    "analyzer_menu_reset_all_widths": "모든 열 너비 초기화",
    "analyzer_menu_manage_columns": "열 관리…",
```
tc:
```json
    "analyzer_menu_sort_asc": "升冪排序",
    "analyzer_menu_sort_desc": "降冪排序",
    "analyzer_menu_hide_column": "隱藏欄位",
    "analyzer_menu_reset_width": "重設欄寬",
    "analyzer_menu_reset_all_widths": "重設所有欄寬",
    "analyzer_menu_manage_columns": "管理欄位…",
```

- [ ] **Step 4.3: Build check** — `cargo check -p ultros-app > /tmp/i18n.log 2>&1; echo "REAL_EXIT=$?"; grep -i "warn\|missing" /tmp/i18n.log | head`. Expected: exit 0, no missing-key warnings. (New keys are unused until Task 8 — that's fine; leptos-i18n only errors on *missing* keys.)

- [ ] **Step 4.4: CI check + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add ultros-frontend/ultros-app/locales/
git commit -m "i18n(flip-finder): sell-world copy + column context-menu strings

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Contained spreadsheet pane

Switch the world view to a fixed-height pane with `fill` container scrolling; move the header into the scroller; delete the scrollLeft mirroring. Widths stay Tailwind-classes for one more task — the stylesheet's `--analyzer-row-min-width` breakpoints still drive row width here, so the page must remain fully working after this task alone.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs`
- Modify: `style/tailwind.css` (comment only in this task)

- [ ] **Step 5.1: Pane-height measurement in `AnalyzerTable`.** Replace the hscroll-sync block (the entire `// --- Horizontal scroll sync ---` section, lines ~875–922, including `header_scroll`, `list_scroll`, `hscroll_listeners`, the `on_cleanup`, and the `Effect`) with:

```rust
    // --- Pane height -------------------------------------------------------
    // The table is a contained pane filling the viewport below the control
    // bar: height = window height − the pane root's document-space top. Both
    // terms are reactive (resize, and any reflow above the pane); the
    // document-space top (viewport top + scroll y) is constant under page
    // scroll, so the pane does not jiggle while the user scrolls to the
    // footer. 0.0 before hydration → the SSR fallback height.
    let pane_root = NodeRef::<leptos::html::Div>::new();
    let pane_bounds = leptos_use::use_element_bounding(pane_root);
    let (_, window_scroll_y) = leptos_use::use_window_scroll();
    let window_size = leptos_use::use_window_size();
    let pane_height = Memo::new(move |_| {
        let window_h = window_size.height.get();
        if window_h <= 0.0 {
            return 640.0; // SSR / pre-hydration fallback
        }
        let doc_top = pane_bounds.top.get() + window_scroll_y.get();
        ((window_h - doc_top) - 8.0).max(320.0)
    });
```

Also delete the now-unused imports at the top of the file: `web_sys::wasm_bindgen::JsCast` and `web_sys::wasm_bindgen::closure::Closure` (check for other users first with a grep in the file — `Closure` is also used by the enrichment debounce/realtime code; only remove if genuinely unused. If still used elsewhere, leave them).

- [ ] **Step 5.2: Fixed-height root.** Change `AnalyzerTable`'s root view element (line ~1251) from:

```rust
        <div class="flex flex-col gap-4">
```

to:

```rust
        <div
            node_ref=pane_root
            class="flex flex-col gap-2 min-h-0"
            style=move || format!("height:{}px;", pane_height().round() as i32)
        >
```

- [ ] **Step 5.3: Table wrapper becomes the flex-filling pane.** Change the table wrapper (line ~1784–1789) from:

```rust
            <div
                class="analyzer-table border border-[color:var(--color-outline)]"
                style=move || {
                    format!("--analyzer-extra-cols: {}px;", extra_column_width_px(&visible_cols()))
                }
            >
```

to:

```rust
            // The pane: fills the rest of the root's fixed height; the
            // VirtualScroller inside it (fill mode) is the single scrollport
            // for both axes, with the column header sticky inside it.
            <div
                class="analyzer-table flex-1 min-h-0 border border-[color:var(--color-outline)]"
                style=move || {
                    format!("--analyzer-extra-cols: {}px;", extra_column_width_px(&visible_cols()))
                }
            >
```

- [ ] **Step 5.4: Scroller switches to fill-container mode.** In the `<VirtualScroller` invocation (line ~1790):
  - Replace `scroll_source=ScrollSource::Window { sticky_offset: STICKY_BAR_HEIGHT }` and `viewport_height=720.0` with:

```rust
                        viewport_height=640.0
                        fill=true
```

  (No `scroll_source` prop: `None` defaults to `ScrollSource::Container { viewport_height }`, and `fill` overrides the height once measured.)
  - Delete the `list_ref=list_scroll` line.
  - In the `header=view! { … }` block, remove the `.analyzer-hscroll` wrapper: delete the line `<div class="analyzer-hscroll" node_ref=header_scroll>` and its matching closing `</div>` (line ~1810 and ~1942), leaving the `.analyzer-grid-row` header div as the direct header content.
  - Update the `header_height` comment (line ~1795–1804): the scrollbar-reservation rationale referenced `.analyzer-hscroll`, which is gone. Replace the comment with:

```rust
                        // The header row's own content height. In fill mode
                        // the header lives inside the single scrollport, so
                        // no scrollbar is reserved on it; `overscan=8`
                        // absorbs any residual off-by-a-few-px.
```

  - Check remaining uses of `STICKY_BAR_HEIGHT` and `ScrollSource` in this file; remove the imports if now unused.

- [ ] **Step 5.5: Sticky bar note.** The control bar div (line ~1255) keeps its classes (`sticky-bar` still provides bg + border-bottom; `position: sticky` is inert now that the page area doesn't scroll past it). Update its comment from the STICKY_BAR_HEIGHT rationale to:

```rust
            // Control bar. Height still fixed so the pane-height measurement
            // is stable; no longer load-bearing for any sticky offset — the
            // table header now sticks inside the pane's own scrollport.
```

- [ ] **Step 5.6: Stylesheet comment.** In `style/tailwind.css`, the block comment at ~2031–2039 ("Flip Finder grid: horizontal scrollports … two *sibling* scrollports … synchronized in analyzer.rs") is now wrong. Replace that comment with:

```css
/* ----- Flip Finder grid -----
   The table is a contained pane (VirtualScroller `fill` mode): the scroller
   is the single scrollport for both axes and the header is sticky inside
   it. `.analyzer-hscroll` survives only as the header's opaque background —
   rows scroll underneath the sticky header. Width comes from
   `--analyzer-row-min-width` on every grid row. */
```

Keep `.analyzer-hscroll` itself for now (deleted in Task 9 once nothing references it).

- [ ] **Step 5.7: Compile** — `cargo check -p ultros-app`; then `cargo test -p ultros-app`. Expected: PASS. Fix any unused-variable warnings from the deleted refs (clippy runs with `-D warnings`).

- [ ] **Step 5.8: Visual smoke test.** Run the e2e harness: `./scripts/run_e2e.sh > /tmp/e2e.log 2>&1; echo "REAL_EXIT=$?"; tail -20 /tmp/e2e.log`. Inspect the flip-finder screenshot in `integration/` output: the table must render inside a pane with its own scrollbars, header visible, rows populated. If the harness can't run in this environment, instead run the dev server and check `/flip-finder/<any world>` manually, verifying: (1) vertical scroll happens inside the pane, page stays put; (2) horizontal scroll moves header and rows together; (3) header stays pinned while rows scroll under it.

- [ ] **Step 5.9: CI check + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add ultros-frontend/ultros-app/src/routes/analyzer.rs style/tailwind.css
git commit -m "feat(flip-finder): contained spreadsheet pane, single scrollport

Header moves inside the VirtualScroller's container-mode sticky slot; the
scrollLeft-mirroring listeners are gone — one scrollport means the mobile
header/body desync cannot happen.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: CSS-variable column widths

Replace the three-place width encoding with the registry: pane carries `--colw-*` vars + calc'd row min-width; every cell sizes with `width: var(--colw-<id>)`. Introduces the `HeaderCell` wrapper that Tasks 7–8 extend. Breakpoint column-hiding (`hidden md:flex` etc.) is removed — visibility is the `?cols=` system's job, and the pane scrolls horizontally on narrow screens.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs`
- Modify: `style/tailwind.css`

- [ ] **Step 6.1: HeaderCell component.** In `analyzer.rs`, next to `SortHeader` (line ~645), add:

```rust
/// One header cell, sized by its column's `--colw-*` variable. Tasks
/// layered on top: the resize handle and the context-menu hookup.
#[component]
fn HeaderCell(
    col: &'static str,
    /// Extra classes: alignment (`justify-end`, `justify-center`) and
    /// anything cell-specific.
    #[prop(optional, into)] class: String,
    children: Children,
) -> impl IntoView {
    view! {
        <div
            role="columnheader"
            class=format!("relative shrink-0 px-3 py-2 flex items-center gap-2 min-w-0 {class}")
            style=format!("width:var(--colw-{col})")
        >
            {children()}
        </div>
    }
    .into_any()
}
```

- [ ] **Step 6.2: Pane style from the registry.** In the table wrapper from Step 5.3, replace the `--analyzer-extra-cols` style with:

```rust
                style=move || colw_style(&visible_cols(), &std::collections::HashMap::new())
```

(Task 7 swaps the empty map for the localStorage overrides.) Delete `extra_column_width_px` (analyzer.rs line ~593–615) and its doc comment.

- [ ] **Step 6.3: Rewrite the header cells** (lines ~1811–1941 region, now inside the scroller's header slot). Every `<div role="columnheader" class="w-… shrink-0 …">` becomes a `HeaderCell`. The full mapping (keep each cell's inner content — `SortHeader`, filter-remove icons, world subtitle spans — exactly as it is today):

```rust
<HeaderCell col=COL_HQ class="!px-2 justify-center">{t!(i18n, analyzer_col_hq)}</HeaderCell>
<HeaderCell col=COL_ITEM>{t!(i18n, analyzer_col_item)}</HeaderCell>
<HeaderCell col=COL_PROFIT class="justify-end">/* SortHeader Profit, unchanged */</HeaderCell>
{move || visible_cols().contains(COL_PROFIT_PER_DAY).then(|| view! {
    <HeaderCell col=COL_PROFIT_PER_DAY class="justify-end">/* SortHeader ProfitPerDay */</HeaderCell>
})}
{move || visible_cols().contains(COL_VELOCITY).then(|| view! {
    <HeaderCell col=COL_VELOCITY class="justify-end">{t!(i18n, analyzer_col_velocity)}</HeaderCell>
})}
{move || visible_cols().contains(COL_DRIFT).then(|| view! {
    <HeaderCell col=COL_DRIFT class="justify-end">{t!(i18n, analyzer_col_drift)}</HeaderCell>
})}
{move || visible_cols().contains(COL_CONFIDENCE).then(|| view! {
    <HeaderCell col=COL_CONFIDENCE class="justify-center">{t!(i18n, analyzer_col_confidence)}</HeaderCell>
})}
{move || visible_cols().contains(COL_ROI).then(|| view! {
    <HeaderCell col=COL_ROI>/* SortHeader Roi */</HeaderCell>
})}
<HeaderCell col=COL_BUY_PRICE class="justify-end">{t!(i18n, analyzer_col_buy_price)}</HeaderCell>
{move || visible_cols().contains(COL_WORLD).then(|| view! {
    <HeaderCell col=COL_WORLD>/* label + filter-remove icon block, unchanged */</HeaderCell>
})}
{move || visible_cols().contains(COL_DATACENTER).then(|| view! {
    <HeaderCell col=COL_DATACENTER>/* label + filter-remove icon block */</HeaderCell>
})}
{move || visible_cols().contains(COL_TREND).then(|| view! {
    <HeaderCell col=COL_TREND class="flex-col justify-center text-center leading-tight !gap-0">/* spark label + world span */</HeaderCell>
})}
{move || visible_cols().contains(COL_SALES_PER_DAY).then(|| view! {
    <HeaderCell col=COL_SALES_PER_DAY class="flex-col justify-center text-center leading-tight !gap-0">/* … */</HeaderCell>
})}
{move || visible_cols().contains(COL_VOLUME_30D).then(|| view! {
    <HeaderCell col=COL_VOLUME_30D class="flex-col items-end text-right leading-tight !gap-0">/* … */</HeaderCell>
})}
{move || visible_cols().contains(COL_LAST_SOLD).then(|| view! {
    <HeaderCell col=COL_LAST_SOLD class="flex-col items-start leading-tight !gap-0">/* … */</HeaderCell>
})}
```

Notes: all `hidden md:flex` / `lg:flex` / `xl:flex` fragments are dropped; the HQ cell's old `w-[44px] px-2 text-center` is expressed as `!px-2 justify-center` (Tailwind important-modifier overrides the base `px-3`); the Item cell loses `flex-1 min-w-[14rem]`.

- [ ] **Step 6.4: Rewrite the row cells** (lines ~1996–2225). Same treatment, but rows stay plain divs (they're the virtualized hot path — no component wrapper). For each cell replace the width/breakpoint classes with `shrink-0 min-w-0` + a `style` attr:

| column | old classes (drop) | new class | new style |
|---|---|---|---|
| HQ | `w-[44px]` | `px-2 py-2 shrink-0 flex items-center justify-center` | `width:var(--colw-hq)` |
| Item | `flex-1 min-w-[14rem]` | `px-4 py-2 flex flex-row items-center gap-2 shrink-0 min-w-0` | `width:var(--colw-item)` |
| Profit | `w-28` | `px-3 py-2 shrink-0 text-right flex items-center justify-end` | `width:var(--colw-profit)` |
| Profit/day | `w-28` | same as Profit | `width:var(--colw-profit_per_day)` |
| Velocity | `w-[88px] hidden md:flex` | `px-3 py-2 shrink-0 flex items-center justify-end font-mono tabular-nums` | `width:var(--colw-velocity)` |
| Drift | `w-[88px] hidden md:flex` | (keep its dynamic `{class}` suffix) | `width:var(--colw-drift)` |
| Confidence | `w-[72px] hidden md:flex` | `px-3 py-2 shrink-0 flex items-center justify-center` | `width:var(--colw-confidence)` |
| ROI | `w-28` | `px-3 py-2 shrink-0 text-right flex items-center justify-end` | `width:var(--colw-roi)` |
| Buy Price | `w-28` | same as Profit | `width:var(--colw-buy_price)` |
| World | `w-28 hidden lg:block` | `px-3 py-2 shrink-0 flex items-center min-w-0` | `width:var(--colw-world)` |
| Datacenter | `w-28 hidden xl:block` | `px-3 py-2 shrink-0 flex items-center min-w-0` | `width:var(--colw-datacenter)` |
| Trend | `w-[100px] hidden md:flex` | `px-3 py-2 shrink-0 flex items-center justify-center` | `width:var(--colw-trend)` |
| Sales/day | `w-[140px] hidden md:flex` | `px-3 py-2 shrink-0 flex items-center justify-center` | `width:var(--colw-sales_per_day)` |
| Volume 30d | `w-[88px] hidden md:flex` | `px-3 py-2 shrink-0 flex items-center justify-end font-mono tabular-nums` | `width:var(--colw-volume_30d)` |
| Last sold | `w-28 hidden md:block` | `px-3 py-2 shrink-0 truncate flex items-center` | `width:var(--colw-last_sold)` |

The Item cell's inner `<a>` keeps `truncate overflow-x-clip min-w-0` — that plus the cell's `min-w-0` is what lets a squeezed name column ellipsize.

- [ ] **Step 6.5: Stylesheet cleanup.** In `style/tailwind.css`, delete the `.analyzer-table` base rule and both `@media` overrides (lines ~2056–2068 — the `--analyzer-row-min-width: calc(38rem …)` family). Keep `.analyzer-grid-row { min-width: var(--analyzer-row-min-width, 0px); }` — it now resolves from the inline calc the pane sets.

- [ ] **Step 6.6: Compile + tests** — `cargo test -p ultros-app`. Expected: PASS (the `colw_style` tests from Task 1 cover the width math).

- [ ] **Step 6.7: Visual check** as in Step 5.8: all columns present at correct widths, horizontal scroll reaches the last column exactly, no dead gutter on the right, name column truncates with ellipsis when the pane is narrow.

- [ ] **Step 6.8: CI check + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add ultros-frontend/ultros-app/src/routes/analyzer.rs style/tailwind.css
git commit -m "refactor(flip-finder): data-driven column widths via CSS variables

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: Drag-resize with localStorage persistence

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs`
- Modify: `style/tailwind.css`

- [ ] **Step 7.1: Width-override storage.** In `AnalyzerTable`, next to the other signal declarations (line ~770 area):

```rust
    // User column-width overrides, px, keyed by column id. Device-local
    // preference like saved views — deliberately NOT in the URL.
    // `delay_during_hydration` is load-bearing (see saved_views.rs).
    let (col_widths, set_col_widths, _) = leptos_use::storage::use_local_storage_with_options::<
        std::collections::HashMap<String, f64>,
        codee::string::JsonSerdeCodec,
    >(
        COL_WIDTHS_KEY,
        leptos_use::storage::UseStorageOptions::default().delay_during_hydration(true),
    );
```

Match the import style of `saved_views.rs` (`use leptos_use::storage::{UseStorageOptions, use_local_storage_with_options};` and `use codee::string::JsonSerdeCodec;` at the top of analyzer.rs).

- [ ] **Step 7.2: Feed overrides into the pane style.** Replace Step 6.2's style closure with:

```rust
                style=move || colw_style(&visible_cols(), &col_widths())
```

- [ ] **Step 7.3: The resize handle.** Add next to `HeaderCell`:

```rust
/// Drag handle on a header cell's right edge. Pointer events + pointer
/// capture give mouse and touch one code path. During the drag the new
/// width is written straight to the pane element's `--colw-*` property
/// (no reactive churn at 60fps); the signal — and through it localStorage
/// — commits once on release, and the pane's reactive `style` re-render
/// then agrees with what the drag already painted.
#[component]
fn ColResizeHandle(
    col: &'static str,
    pane: NodeRef<leptos::html::Div>,
    col_widths: Signal<std::collections::HashMap<String, f64>>,
    set_col_widths: WriteSignal<std::collections::HashMap<String, f64>>,
) -> impl IntoView {
    // (start_client_x, width_at_start)
    let drag = RwSignal::new(None::<(f64, f64)>);
    let spec = column_spec(col).expect("resize handle on unregistered column");

    let width_from = move |ev: &web_sys::PointerEvent| -> Option<f64> {
        let (start_x, start_w) = drag.get_untracked()?;
        Some((start_w + (ev.client_x() as f64 - start_x)).max(spec.min_width))
    };

    view! {
        <div
            class="analyzer-col-resize"
            on:pointerdown=move |ev: web_sys::PointerEvent| {
                ev.prevent_default();
                ev.stop_propagation();
                let target: web_sys::HtmlElement =
                    event_target::<web_sys::HtmlElement>(&ev);
                let _ = target.set_pointer_capture(ev.pointer_id());
                let start_w = effective_width(spec, &col_widths.get_untracked());
                drag.set(Some((ev.client_x() as f64, start_w)));
            }
            on:pointermove=move |ev: web_sys::PointerEvent| {
                if let (Some(w), Some(el)) = (width_from(&ev), pane.get_untracked()) {
                    let _ = el
                        .style()
                        .set_property(&format!("--colw-{col}"), &format!("{}px", w.round()));
                }
            }
            on:pointerup=move |ev: web_sys::PointerEvent| {
                if let Some(w) = width_from(&ev) {
                    set_col_widths.update(|m| {
                        m.insert(col.to_string(), w.round());
                    });
                }
                drag.set(None);
            }
            on:pointercancel=move |_| drag.set(None)
            on:dblclick=move |_| {
                // Double-click a handle = reset that column to its default.
                set_col_widths.update(|m| {
                    m.remove(col);
                });
            }
        ></div>
    }
    .into_any()
}
```

(If `event_target::<web_sys::HtmlElement>` doesn't fit the existing import set, use `ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())` with the already-imported `JsCast`.)

- [ ] **Step 7.4: Mount handles in `HeaderCell`.** Extend `HeaderCell` with the props and render the handle after the children:

```rust
#[component]
fn HeaderCell(
    col: &'static str,
    #[prop(optional, into)] class: String,
    pane: NodeRef<leptos::html::Div>,
    col_widths: Signal<std::collections::HashMap<String, f64>>,
    set_col_widths: WriteSignal<std::collections::HashMap<String, f64>>,
    children: Children,
) -> impl IntoView {
    let resizable = column_spec(col).map(|s| s.resizable).unwrap_or(false);
    view! {
        <div
            role="columnheader"
            class=format!("relative shrink-0 px-3 py-2 flex items-center gap-2 min-w-0 {class}")
            style=format!("width:var(--colw-{col})")
        >
            {children()}
            {resizable.then(|| view! {
                <ColResizeHandle col pane col_widths set_col_widths />
            })}
        </div>
    }
    .into_any()
}
```

Every `HeaderCell` call site from Task 6 gains `pane=pane_ref col_widths=col_widths set_col_widths=set_col_widths`. Add `node_ref=pane_ref` to the `.analyzer-table` pane div (Step 5.3's element) — reuse a new `let pane_ref = NodeRef::<leptos::html::Div>::new();` declared beside the storage signal (NOT the `pane_root` from Task 5, which is the outer fixed-height box; the vars live on `.analyzer-table`).

- [ ] **Step 7.5: Handle CSS.** In `style/tailwind.css`, after `.analyzer-grid-row`:

```css
/* Column-resize handle: a 12px hit area straddling the header cell's right
   edge (half over the neighbor — standard spreadsheet affordance), with a
   1px visual line that thickens on hover/drag. `touch-action: none` is
   what makes pointer-capture drags work on touch instead of panning the
   pane. */
.analyzer-col-resize {
    position: absolute;
    top: 0;
    bottom: 0;
    right: -6px;
    width: 12px;
    cursor: col-resize;
    touch-action: none;
    z-index: 5;
}
.analyzer-col-resize::after {
    content: "";
    position: absolute;
    top: 15%;
    bottom: 15%;
    left: 5px;
    width: 1px;
    background: var(--color-outline);
}
.analyzer-col-resize:hover::after,
.analyzer-col-resize:active::after {
    left: 4px;
    width: 3px;
    background: var(--brand-ring);
}
```

- [ ] **Step 7.6: Compile + tests** — `cargo test -p ultros-app`. Expected: PASS.

- [ ] **Step 7.7: Manual verification** (dev server): drag a handle — column resizes live, row min-width follows (last column stays exactly reachable); release — reload the page and the width survives; double-click the handle — width resets; squeeze Item to its min — name ellipsizes. On a touch device/emulation: drag works without panning the pane.

- [ ] **Step 7.8: CI check + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add ultros-frontend/ultros-app/src/routes/analyzer.rs style/tailwind.css
git commit -m "feat(flip-finder): drag-resizable columns persisted to localStorage

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 8: Header context menu

Right-click (or touch long-press) a header → anchored menu: sort asc/desc (sortable columns), hide (optional columns), reset width(s), manage columns.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs`
- Modify: `style/tailwind.css`

- [ ] **Step 8.1: Menu state + sort mapping.** Near `HeaderCell` in analyzer.rs:

```rust
#[derive(Clone, Copy, PartialEq)]
struct HeaderMenuState {
    col: &'static str,
    /// Viewport (client) coordinates of the triggering event; the menu is
    /// `position: fixed` so these are used directly.
    x: f64,
    y: f64,
}

/// Which SortMode a column header sorts by, if any. Only these three exist;
/// adding sort modes for other columns is out of scope (spec).
fn sort_mode_for_col(col: &str) -> Option<SortMode> {
    match col {
        c if c == COL_PROFIT => Some(SortMode::Profit),
        c if c == COL_PROFIT_PER_DAY => Some(SortMode::ProfitPerDay),
        c if c == COL_ROI => Some(SortMode::Roi),
        _ => None,
    }
}
```

- [ ] **Step 8.2: Open from `HeaderCell`.** Add a `menu: RwSignal<Option<HeaderMenuState>>` prop to `HeaderCell`, and on the cell div add:

```rust
            on:contextmenu=move |ev: web_sys::MouseEvent| {
                ev.prevent_default();
                menu.set(Some(HeaderMenuState {
                    col,
                    x: ev.client_x() as f64,
                    y: ev.client_y() as f64,
                }));
            }
```

Long-press for touch (iOS Safari fires no `contextmenu` on long-press): add to `HeaderCell`:

```rust
    // Touch long-press → same menu. Canceled by lift-off or movement (a
    // drag/scroll is not a long-press).
    let longpress = StoredValue::new_local(None::<leptos::leptos_dom::helpers::TimeoutHandle>);
    let cancel_longpress = move || {
        longpress.update_value(|h| {
            if let Some(h) = h.take() {
                h.clear();
            }
        });
    };
```

and on the cell div:

```rust
            on:pointerdown=move |ev: web_sys::PointerEvent| {
                if ev.pointer_type() == "touch" {
                    let (x, y) = (ev.client_x() as f64, ev.client_y() as f64);
                    let handle = leptos::leptos_dom::helpers::set_timeout_with_handle(
                        move || menu.set(Some(HeaderMenuState { col, x, y })),
                        std::time::Duration::from_millis(500),
                    )
                    .ok();
                    longpress.set_value(handle);
                }
            }
            on:pointerup=move |_| cancel_longpress()
            on:pointercancel=move |_| cancel_longpress()
            on:pointermove=move |_| cancel_longpress()
```

- [ ] **Step 8.3: The menu component.** Add:

```rust
#[component]
fn HeaderContextMenu(
    menu: RwSignal<Option<HeaderMenuState>>,
    visible_cols: Memo<std::collections::HashSet<&'static str>>,
    set_cols_param: SignalSetter<Option<String>>,
    col_widths: Signal<std::collections::HashMap<String, f64>>,
    set_col_widths: WriteSignal<std::collections::HashMap<String, f64>>,
    set_sort_mode: SignalSetter<Option<SortMode>>,
    set_sort_dir: SignalSetter<Option<SortDir>>,
    show_columns_picker: RwSignal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let node = NodeRef::<leptos::html::Div>::new();
    let _ = leptos_use::on_click_outside(node, move |_| menu.set(None));
    // Escape closes; capture-phase scroll closes (pane scrolling happens on
    // an element, so a bubbling listener on window would never see it).
    let _ = leptos_use::use_event_listener(
        leptos_use::use_window(),
        leptos::ev::keydown,
        move |ev| {
            if ev.key() == "Escape" {
                menu.set(None);
            }
        },
    );
    let _ = leptos_use::use_event_listener_with_options(
        leptos_use::use_window(),
        leptos::ev::scroll,
        move |_| menu.set(None),
        leptos_use::UseEventListenerOptions::default().capture(true),
    );

    move || {
        menu.get().map(|state| {
            let col = state.col;
            let spec = column_spec(col);
            let sortable = sort_mode_for_col(col);
            let optional = spec.map(|s| s.optional).unwrap_or(false);
            let has_override = col_widths.with(|m| m.contains_key(col));
            let close = move || menu.set(None);
            // Keep the menu on-screen near the right edge.
            let style = format!(
                "left:min({}px, calc(100vw - 15rem));top:{}px;",
                state.x, state.y
            );
            view! {
                <div node_ref=node class="analyzer-context-menu" style=style role="menu">
                    {sortable
                        .map(|mode| {
                            view! {
                                <button
                                    class="analyzer-context-item"
                                    role="menuitem"
                                    on:click=move |_| {
                                        set_sort_mode(Some(mode));
                                        set_sort_dir(None); // desc is the URL-clean default
                                        close();
                                    }
                                >
                                    {t!(i18n, analyzer_menu_sort_desc)}
                                </button>
                                <button
                                    class="analyzer-context-item"
                                    role="menuitem"
                                    on:click=move |_| {
                                        set_sort_mode(Some(mode));
                                        set_sort_dir(Some(SortDir::Asc));
                                        close();
                                    }
                                >
                                    {t!(i18n, analyzer_menu_sort_asc)}
                                </button>
                            }
                        })}
                    {optional
                        .then(|| {
                            view! {
                                <button
                                    class="analyzer-context-item"
                                    role="menuitem"
                                    on:click=move |_| {
                                        let mut set = visible_cols.get_untracked();
                                        set.remove(col);
                                        set_cols_param.set(Some(serialize_visible_cols(&set)));
                                        close();
                                    }
                                >
                                    {t!(i18n, analyzer_menu_hide_column)}
                                </button>
                            }
                        })}
                    {has_override
                        .then(|| {
                            view! {
                                <button
                                    class="analyzer-context-item"
                                    role="menuitem"
                                    on:click=move |_| {
                                        set_col_widths.update(|m| {
                                            m.remove(col);
                                        });
                                        close();
                                    }
                                >
                                    {t!(i18n, analyzer_menu_reset_width)}
                                </button>
                            }
                        })}
                    <button
                        class="analyzer-context-item"
                        role="menuitem"
                        on:click=move |_| {
                            set_col_widths.update(|m| m.clear());
                            close();
                        }
                    >
                        {t!(i18n, analyzer_menu_reset_all_widths)}
                    </button>
                    <div class="my-1 border-t border-[color:var(--color-outline)]" />
                    <button
                        class="analyzer-context-item"
                        role="menuitem"
                        on:click=move |_| {
                            show_columns_picker.set(true);
                            close();
                        }
                    >
                        {t!(i18n, analyzer_menu_manage_columns)}
                    </button>
                </div>
            }
        })
    }
}
```

Check `leptos_use` exports for the exact names (`use_event_listener_with_options`, `UseEventListenerOptions` — tooltip.rs already imports `use_event_listener_with_options`, follow its pattern). If wiring `use_window()` proves awkward, `window()` (Leptos helper) as the target is fine.

- [ ] **Step 8.4: Wire it up in `AnalyzerTable`.** Beside the other signals: `let header_menu = RwSignal::new(None::<HeaderMenuState>);`. Rename `_set_sort_mode`/`_set_sort_dir` (line ~746–747) to `set_sort_mode`/`set_sort_dir`. Pass `menu=header_menu` to every `HeaderCell`. Mount the menu once, directly after the `.analyzer-table` pane div closes (inside the root fixed-height div — it's `position: fixed`, placement in the tree only matters for not being inside the scroller, where it would be clipped):

```rust
            <HeaderContextMenu
                menu=header_menu
                visible_cols
                set_cols_param
                col_widths
                set_col_widths
                set_sort_mode
                set_sort_dir
                show_columns_picker
            />
```

- [ ] **Step 8.5: Menu CSS.** In `style/tailwind.css`, after the resize-handle rules:

```css
/* Header context menu: fixed-position, viewport-anchored (the pane scroller
   would clip an absolutely-positioned child). */
.analyzer-context-menu {
    position: fixed;
    z-index: 50;
    min-width: 13rem;
    padding: 0.375rem;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--color-outline);
    border-radius: 0.5rem;
    background-color: var(--color-background-panel);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
}
.analyzer-context-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    text-align: left;
    padding: 0.375rem 0.625rem;
    font-size: 0.85rem;
    border-radius: 0.375rem;
    color: var(--color-text);
    cursor: pointer;
}
.analyzer-context-item:hover {
    background-color: color-mix(in srgb, var(--brand-ring) 14%, transparent);
}
```

- [ ] **Step 8.6: Compile + tests** — `cargo test -p ultros-app`. Expected: PASS.

- [ ] **Step 8.7: Manual verification**: right-click Profit header → menu shows Sort desc/asc, Reset all widths, Manage columns (no Hide — required column); click Sort ascending → `?sort=profit&dir=asc`, rows flip; right-click Velocity → Hide column removes it and `?cols=` updates; Reset width appears only after a resize; Manage columns opens the picker; Escape and outside-click close; long-press on touch emulation opens it.

- [ ] **Step 8.8: CI check + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add ultros-frontend/ultros-app/src/routes/analyzer.rs style/tailwind.css
git commit -m "feat(flip-finder): header context menu — sort, hide, reset widths

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 9: Spreadsheet styling + dead CSS removal

**Files:**
- Modify: `style/tailwind.css`
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (header row classes only)

- [ ] **Step 9.1: Gridlines.** In `style/tailwind.css`, with the other analyzer rules:

```css
/* Spreadsheet gridlines: hairline column separators on every cell, hairline
   row borders. Rows are 40px border-box (h-10), so the border eats into the
   row's own height and the virtualizer's fixed row_height stays exact. */
.analyzer-table [role="cell"],
.analyzer-table [role="columnheader"] {
    border-right: 1px solid color-mix(in srgb, var(--color-outline) 55%, transparent);
}
.analyzer-grid-row > :last-child {
    border-right: none;
}
.analyzer-table .analyzer-grid-row {
    border-bottom: 1px solid color-mix(in srgb, var(--color-outline) 35%, transparent);
}
```

- [ ] **Step 9.2: Header row.** In the header's `.analyzer-grid-row` div (analyzer.rs, inside the scroller's header slot), replace `border-b border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)]` with `bg-[color:var(--color-background-panel)]` — the sticky header needs an opaque background now that rows scroll under it inside the pane (the old translucent brand tint over transparent shows rows through). Keep the height/typography classes as they are, and layer the tint *over* the opaque color by adding back `bg-[image:linear-gradient(color-mix(in_srgb,var(--brand-ring)_8%,transparent),color-mix(in_srgb,var(--brand-ring)_8%,transparent))]` only if the flat panel color looks off in the visual check — flat is the default choice.

- [ ] **Step 9.3: Delete dead CSS.** Remove the `.analyzer-hscroll` rule block entirely (nothing references the class after Task 5), and re-check the section comment from Step 5.6 still describes reality (it should now mention the header's opacity comes from the header row itself).

- [ ] **Step 9.4: Visual check** (Step 5.8 method): gridlines visible but subtle in both light and dark themes; header opaque while rows scroll beneath; alternating row striping still reads; last column has no trailing separator.

- [ ] **Step 9.5: CI check + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add style/tailwind.css ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "style(flip-finder): spreadsheet gridlines, opaque sticky header

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 10: Final verification

- [ ] **Step 10.1: Full test suite** — `cargo test -p ultros-app > /tmp/final.log 2>&1; echo "REAL_EXIT=$?"; tail -10 /tmp/final.log`. Expected: `REAL_EXIT=0`.

- [ ] **Step 10.2: Full CI** — `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log`. Expected: `REAL_EXIT=0`.

- [ ] **Step 10.3: E2E smoke** — `./scripts/run_e2e.sh > /tmp/e2e.log 2>&1; echo "REAL_EXIT=$?"; tail -20 /tmp/e2e.log`. Review flip-finder screenshots.

- [ ] **Step 10.4: Manual checklist** (dev server, desktop + mobile emulation):
  - Fresh visit to `/flip-finder/<world>` with a clean URL → Realistic chips appear (`min-buy=5000`, `last-sold=1d`, `roi=30`, `next-sale=1d`) and sort is profit/day; a URL with `?profit=50000` gets NO extra chips.
  - `/flip-finder` landing page + world view both say the sell-world copy.
  - Pane scrolls both axes as one unit on mobile emulation — drag diagonally, header tracks columns exactly.
  - Resize, hide via context menu, hide via picker, restore via picker, reload — widths persist, `?cols=` round-trips.
  - Clear-all filters works; saved views still apply; clicking a built-in view navigates correctly.
  - Landing page (`/flip-finder`) unaffected structurally.

- [ ] **Step 10.5: Update the spec's status line** in `docs/superpowers/specs/2026-07-30-flip-finder-spreadsheet-design.md` from "pending spec review" to "implemented", commit:

```bash
git add docs/superpowers/specs/2026-07-30-flip-finder-spreadsheet-design.md
git commit -m "docs: mark flip finder spreadsheet spec implemented

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```
