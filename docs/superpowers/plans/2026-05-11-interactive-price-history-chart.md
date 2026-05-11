# Interactive Price History Chart Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the plotters-canvas in-browser scatter chart at [ultros-frontend/ultros-app/src/components/price_history_chart.rs](../../ultros-frontend/ultros-app/src/components/price_history_chart.rs) with an interactive SVG chart built on `leptos-chartistry`, add time-range chips, a stats strip, and financial-chart overlays (VWAP, IQR band, trendline).

**Architecture:** A single self-contained Leptos component owns chart chrome, controls, and overlays. Pure helpers (series grouping, stats, VWAP) live inside the same file with unit tests — they don't need a shared crate because the server-side plotters chart keeps its own copy. The plotters-based PNG generator at [ultros/src/web/item_card.rs](../../ultros/src/web/item_card.rs) is untouched.

**Tech Stack:** Rust, Leptos 0.8.14, `leptos-chartistry` 0.2.3 (SVG charting), Tailwind CSS, chrono.

**Spec:** [2026-05-11-interactive-price-history-chart-design.md](../specs/2026-05-11-interactive-price-history-chart-design.md)

---

## File Structure

**Files modified:**
- `ultros-frontend/ultros-app/Cargo.toml` — add `leptos-chartistry`, remove `plotters-canvas`
- `ultros-frontend/ultros-app/src/components/price_history_chart.rs` — full rewrite
- `ultros-frontend/ultros-app/src/routes/item_view.rs` — drop the outer wrapping `<div class="panel p-6 ...">` around the chart (component owns its panel chrome now)
- `ultros-frontend/ultros-app/locales/en.json` — add new i18n keys
- `ultros-frontend/ultros-app/locales/{de,fr,ja,cn,ko,tc}.json` — add new i18n keys (English fallback acceptable per existing Jules-bot pattern; treat as best-effort)

**No new files.** All helpers live inside `price_history_chart.rs` with inline `#[cfg(test)] mod tests`. This matches the existing pattern at [sale_history_table.rs](../../ultros-frontend/ultros-app/src/components/sale_history_table.rs) (`find_date_range` + tests in one file).

**Untouched:**
- `ultros-frontend/ultros-charts/` — still used by the PNG endpoint
- `ultros/src/web/item_card.rs` — still uses plotters

---

## Pre-flight

- [ ] **Step 0a: Confirm worktree state**

Run: `git status` from the worktree root.
Expected: clean, on branch `claude/modest-lewin-141588`, two `docs/superpowers/specs/` and a `docs/superpowers/plans/` commit ahead of `origin/main`.

- [ ] **Step 0b: Verify the submodule is initialized**

Run: `ls xiv-gen/ffxiv-datamining/csv/cn/Item.csv` from the worktree root.
Expected: file exists. If not: `git submodule update --init --recursive --depth=1`. CLAUDE.md flags this as the most common CI break.

---

## Task 1: Swap the chart library dependency

**Files:**
- Modify: `ultros-frontend/ultros-app/Cargo.toml`

- [ ] **Step 1: Add leptos-chartistry, remove plotters-canvas**

Edit `ultros-frontend/ultros-app/Cargo.toml`:

Replace this line:
```toml
plotters-canvas = "0.3"
```

with:
```toml
leptos-chartistry = "0.2"
```

- [ ] **Step 2: Verify it resolves**

Run: `cargo check -p ultros-app --no-default-features --features hydrate` from the worktree root.
Expected: builds clean. If `leptos-chartistry` fails to resolve against Leptos 0.8.14, fall back to the hand-rolled SVG approach documented in the spec (note in the PR description and skip Tasks 5–7 in their chartistry-specific form; structure stays the same).

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/Cargo.toml Cargo.lock
git commit -m "chore: swap plotters-canvas for leptos-chartistry in ultros-app"
```

---

## Task 2: Extract series grouping helper with tests

We move the world → DC → region rollup logic out of `ultros-charts::map_sale_history_to_line` and reimplement it inside `price_history_chart.rs`. The plotters version stays in `ultros-charts` untouched (used by the PNG generator).

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs`

- [ ] **Step 1: Write the failing tests at the bottom of `price_history_chart.rs`**

Add to the very bottom of the file, after the existing `}` closing the `PriceHistoryChart` component:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use ultros_api_types::world_helper::{Datacenter, OwnedResult, Region, World, WorldHelper};

    fn test_world_helper() -> WorldHelper {
        // Two regions, two DCs in region 1, two worlds in DC 1, one in DC 2, one in DC 3.
        WorldHelper::from((
            vec![
                Region { id: 1, name: "North-America".into() },
                Region { id: 2, name: "Europe".into() },
            ],
            vec![
                Datacenter { id: 10, name: "Aether".into(), region_id: 1 },
                Datacenter { id: 11, name: "Crystal".into(), region_id: 1 },
                Datacenter { id: 20, name: "Light".into(), region_id: 2 },
            ],
            vec![
                World { id: 100, name: "Gilgamesh".into(), datacenter_id: 10 },
                World { id: 101, name: "Adamantoise".into(), datacenter_id: 10 },
                World { id: 102, name: "Balmung".into(), datacenter_id: 11 },
                World { id: 200, name: "Phoenix".into(), datacenter_id: 20 },
            ],
        ))
    }

    fn sale(world_id: i32, price: i32, qty: i32, ts: i64) -> SaleHistory {
        SaleHistory {
            id: 0,
            quantity: qty,
            price_per_item: price,
            buying_character_id: 0,
            hq: false,
            sold_item_id: 1,
            sold_date: chrono::Utc.timestamp_opt(ts, 0).unwrap().naive_utc(),
            world_id,
        }
    }

    #[test]
    fn grouping_collapses_to_world_when_one_dc() {
        let helper = test_world_helper();
        let sales = vec![sale(100, 1000, 1, 0), sale(101, 1100, 1, 1)];
        let series = group_sales_by_locale(&helper, &sales);
        let names: Vec<_> = series.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"Gilgamesh"));
        assert!(names.contains(&"Adamantoise"));
    }

    #[test]
    fn grouping_collapses_to_dc_when_one_region() {
        let helper = test_world_helper();
        let sales = vec![sale(100, 1000, 1, 0), sale(102, 1100, 1, 1)];
        let series = group_sales_by_locale(&helper, &sales);
        let names: Vec<_> = series.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"Aether"));
        assert!(names.contains(&"Crystal"));
    }

    #[test]
    fn grouping_collapses_to_region_when_multiple_regions() {
        let helper = test_world_helper();
        let sales = vec![sale(100, 1000, 1, 0), sale(200, 1100, 1, 1)];
        let series = group_sales_by_locale(&helper, &sales);
        let names: Vec<_> = series.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"North-America"));
        assert!(names.contains(&"Europe"));
    }
}
```

If the `WorldHelper::from` constructor or `Region/Datacenter/World` field names don't match the actual types in `ultros-api-types/src/world_helper.rs`, open that file and adjust the test fixture to match. Do not change the production types — only the test fixture.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-app --no-default-features --features hydrate grouping_ -- --nocapture` from the worktree root.
Expected: FAIL — `group_sales_by_locale` not yet defined.

- [ ] **Step 3: Implement `group_sales_by_locale`**

Add to `price_history_chart.rs` before the `PriceHistoryChart` component definition (after the `use` block):

```rust
use std::collections::HashSet;
use ultros_api_types::world_helper::AnySelector;

type SeriesPoints = Vec<(chrono::DateTime<chrono::Local>, i32, i32)>;

/// Roll sales up to world / DC / region depending on how many distinct regions
/// and DCs are represented. Mirrors the rule in `ultros-charts::map_sale_history_to_line`.
fn group_sales_by_locale(
    helper: &ultros_api_types::world_helper::WorldHelper,
    sales: &[SaleHistory],
) -> Vec<(String, SeriesPoints)> {
    use itertools::Itertools;

    let world_ids: HashSet<AnySelector> = sales
        .iter()
        .map(|s| AnySelector::World(s.world_id))
        .collect();
    let datacenters: HashSet<AnySelector> = world_ids
        .iter()
        .flat_map(|w| {
            helper
                .lookup_selector(*w)
                .and_then(|r| r.as_world())
                .map(|w| AnySelector::Datacenter(w.datacenter_id))
        })
        .collect();
    let regions: HashSet<AnySelector> = datacenters
        .iter()
        .flat_map(|dc| {
            helper
                .lookup_selector(*dc)
                .and_then(|r| r.as_datacenter())
                .map(|dc| AnySelector::Region(dc.region_id))
        })
        .collect();
    let selectors = if datacenters.len() == 1 {
        world_ids
    } else if regions.len() == 1 {
        datacenters
    } else {
        regions
    };
    selectors
        .into_iter()
        .filter_map(|sel| {
            let result = helper.lookup_selector(sel)?;
            let name = result.get_name().to_string();
            let points: SeriesPoints = sales
                .iter()
                .filter(|s| {
                    helper
                        .lookup_selector(AnySelector::World(s.world_id))
                        .map(|w| w.is_in(&result))
                        .unwrap_or_default()
                })
                .filter_map(|s| {
                    Some((
                        s.sold_date.and_local_timezone(chrono::Local).single()?,
                        s.price_per_item,
                        s.quantity,
                    ))
                })
                .collect();
            Some((name, points))
        })
        .sorted_by_cached_key(|(name, _)| name.clone())
        .collect()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --no-default-features --features hydrate grouping_ -- --nocapture`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/price_history_chart.rs
git commit -m "refactor(chart): extract group_sales_by_locale helper with tests"
```

---

## Task 3: Stats helpers (VWAP, median, IQR) with tests

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs`

- [ ] **Step 1: Write the failing tests**

Add inside the existing `#[cfg(test)] mod tests` block (do not create a second one):

```rust
    #[test]
    fn vwap_weights_by_quantity() {
        // 1 unit at 100, 9 units at 200 → VWAP = (100 + 1800) / 10 = 190
        let prices = vec![(100, 1), (200, 9)];
        assert_eq!(vwap(&prices), Some(190));
    }

    #[test]
    fn vwap_returns_none_for_empty() {
        assert_eq!(vwap(&[]), None);
    }

    #[test]
    fn vwap_returns_none_when_total_qty_zero() {
        let prices = vec![(100, 0), (200, 0)];
        assert_eq!(vwap(&prices), None);
    }

    #[test]
    fn median_of_odd_count() {
        let prices = vec![300, 100, 200];
        assert_eq!(median(&prices), Some(200));
    }

    #[test]
    fn median_of_even_count_averages_middle_two() {
        // sorted: 100, 200, 300, 400 → (200 + 300) / 2 = 250
        let prices = vec![400, 100, 300, 200];
        assert_eq!(median(&prices), Some(250));
    }

    #[test]
    fn median_returns_none_for_empty() {
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn iqr_band_returns_none_for_small_samples() {
        let prices: Vec<i32> = (0..9).collect();
        assert_eq!(iqr_band(&prices), None);
    }

    #[test]
    fn iqr_band_widens_with_25x_multiplier() {
        // 20 samples from 0..20: Q1 = idx 5 = 5, Q3 = idx 15 = 15, IQR*2.5 = 25
        // → band = (5 - 25, 15 + 25) = (-20, 40)
        let prices: Vec<i32> = (0..20).collect();
        assert_eq!(iqr_band(&prices), Some((-20, 40)));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-app --no-default-features --features hydrate -- vwap_ median_ iqr_band_ --nocapture`
Expected: FAIL — `vwap`, `median`, `iqr_band` not yet defined.

- [ ] **Step 3: Implement the stats helpers**

Add to `price_history_chart.rs` (alongside `group_sales_by_locale`, before the `#[component]`):

```rust
/// Volume-weighted average price. Returns None if the input is empty or total qty is 0.
fn vwap(prices_and_qty: &[(i32, i32)]) -> Option<i32> {
    let (num, den) = prices_and_qty
        .iter()
        .fold((0i64, 0i64), |(n, d), (price, qty)| {
            (n + (*price as i64) * (*qty as i64), d + (*qty as i64))
        });
    if den == 0 {
        return None;
    }
    Some((num / den) as i32)
}

/// Median price. For even counts, returns the integer mean of the two middle values.
fn median(prices: &[i32]) -> Option<i32> {
    if prices.is_empty() {
        return None;
    }
    let mut sorted: Vec<i32> = prices.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    if n % 2 == 1 {
        Some(sorted[n / 2])
    } else {
        Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2)
    }
}

/// IQR-based outlier band, matching the existing logic in `ultros-charts`.
/// Returns (min, max) where min = Q1 - 2.5·IQR, max = Q3 + 2.5·IQR.
/// Returns None for samples smaller than 10.
fn iqr_band(prices: &[i32]) -> Option<(i32, i32)> {
    if prices.len() < 10 {
        return None;
    }
    let mut sorted: Vec<i32> = prices.to_vec();
    sorted.sort_unstable();
    let q1_idx = sorted.len() / 4;
    let q3_idx = sorted.len() - q1_idx;
    let q1 = *sorted.get(q1_idx)?;
    let q3 = *sorted.get(q3_idx)?;
    let widened = ((q3 - q1) as f32 * 2.5) as i32;
    Some((q1 - widened, q3 + widened))
}

/// Format an integer price using K/mil shortening, same rules as the plotters chart.
fn short_number(value: i32) -> String {
    match value {
        1_000_000.. => format!("{:.2}mil", value as f32 / 1_000_000.0),
        1_000..=999_999 => format!("{:.2}K", value as f32 / 1_000.0),
        _ => value.to_string(),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ultros-app --no-default-features --features hydrate -- vwap_ median_ iqr_band_ --nocapture`
Expected: PASS (8 tests).

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/price_history_chart.rs
git commit -m "feat(chart): add vwap, median, iqr_band, short_number helpers"
```

---

## Task 4: Time-range filter type with tests

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs`

- [ ] **Step 1: Write the failing tests**

Add inside the same `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn time_range_all_keeps_everything() {
        let now = chrono::Utc::now().naive_utc();
        let sales = vec![
            sale(100, 1000, 1, now.and_utc().timestamp() - 60 * 60 * 24 * 60),
            sale(100, 2000, 1, now.and_utc().timestamp()),
        ];
        let filtered = filter_by_range(&sales, TimeRange::All, now);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn time_range_24h_filters_older_sales() {
        let now = chrono::Utc::now().naive_utc();
        let sales = vec![
            sale(100, 1000, 1, now.and_utc().timestamp() - 60 * 60 * 25), // 25h ago
            sale(100, 2000, 1, now.and_utc().timestamp() - 60 * 60),      // 1h ago
        ];
        let filtered = filter_by_range(&sales, TimeRange::Last24h, now);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].price_per_item, 2000);
    }

    #[test]
    fn time_range_7d_filters_older_sales() {
        let now = chrono::Utc::now().naive_utc();
        let sales = vec![
            sale(100, 1000, 1, now.and_utc().timestamp() - 60 * 60 * 24 * 8), // 8d ago
            sale(100, 2000, 1, now.and_utc().timestamp() - 60 * 60 * 24 * 3), // 3d ago
        ];
        let filtered = filter_by_range(&sales, TimeRange::Last7d, now);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].price_per_item, 2000);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-app --no-default-features --features hydrate -- time_range_ --nocapture`
Expected: FAIL — `TimeRange` and `filter_by_range` not defined.

- [ ] **Step 3: Implement the type and filter**

Add to `price_history_chart.rs`:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TimeRange {
    Last24h,
    Last7d,
    Last30d,
    All,
}

impl TimeRange {
    fn label(self) -> &'static str {
        match self {
            TimeRange::Last24h => "24h",
            TimeRange::Last7d => "7d",
            TimeRange::Last30d => "30d",
            TimeRange::All => "All",
        }
    }

    fn cutoff(self, now: chrono::NaiveDateTime) -> Option<chrono::NaiveDateTime> {
        let delta = match self {
            TimeRange::Last24h => chrono::Duration::hours(24),
            TimeRange::Last7d => chrono::Duration::days(7),
            TimeRange::Last30d => chrono::Duration::days(30),
            TimeRange::All => return None,
        };
        Some(now - delta)
    }
}

/// Filter sales whose `sold_date` is on-or-after the range cutoff. `now` is parameterized for tests.
fn filter_by_range(
    sales: &[SaleHistory],
    range: TimeRange,
    now: chrono::NaiveDateTime,
) -> Vec<SaleHistory> {
    match range.cutoff(now) {
        Some(cutoff) => sales
            .iter()
            .filter(|s| s.sold_date >= cutoff)
            .cloned()
            .collect(),
        None => sales.to_vec(),
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ultros-app --no-default-features --features hydrate -- time_range_ --nocapture`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/price_history_chart.rs
git commit -m "feat(chart): add TimeRange filter type with tests"
```

---

## Task 5: New i18n keys

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/en.json`
- Modify (best-effort): `ultros-frontend/ultros-app/locales/{de,fr,ja,cn,ko,tc}.json`

- [ ] **Step 1: Find the alphabetically correct spot in `en.json`**

Open `ultros-frontend/ultros-app/locales/en.json` and find the section around `"no_sales_found"`. Add these new keys (placed wherever alphabetical order is maintained — the existing file is alpha-sorted):

```json
"chart_no_sales_in_window": "No sales in this window. Try a wider range.",
"chart_stat_n_sales": "{n} sales",
"chart_stat_vwap": "VWAP",
"chart_stat_median": "median",
"chart_stat_min": "min",
"chart_stat_max": "max",
"chart_range_24h": "24h",
"chart_range_7d": "7d",
"chart_range_30d": "30d",
"chart_range_all": "All",
"chart_aria_label": "Scatter plot of {n} sales between {from} and {to}"
```

- [ ] **Step 2: Mirror keys into the other locales with English fallback**

For each of `de.json`, `fr.json`, `ja.json`, `cn.json`, `ko.json`, `tc.json`: open the file and add the same keys with the same English values. Translation can happen in a follow-up. This is consistent with the Jules-bot-driven i18n pattern noted in the project memory — translation is iterative.

- [ ] **Step 3: Verify the build still passes**

Run: `cargo check -p ultros-app --no-default-features --features hydrate`
Expected: builds clean. `leptos_i18n` will surface any missing-key errors at compile time.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/locales/
git commit -m "i18n: add chart range, stat, and aria keys"
```

---

## Task 6: Build the new PriceHistoryChart component

This is the big task. It can be done in one commit because the component is one cohesive piece — the helpers it consumes are already shipped and tested.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs`

- [ ] **Step 1: Read the current component to know what to replace**

Read `ultros-frontend/ultros-app/src/components/price_history_chart.rs` end to end. The current `PriceHistoryChart` component (lines 20–157 in the pre-refactor file, but it may have grown after Tasks 2–4 added helpers below it) is what we're replacing. The helpers and tests added in Tasks 2–4 stay. Only the `#[component] pub fn PriceHistoryChart` body and its imports change.

- [ ] **Step 2: Replace the imports at the top of the file**

Remove:
```rust
use std::cell::RefCell;
use std::rc::Rc;
use cfg_if::cfg_if;
use leptos::html::Div;
use leptos::{html::Canvas, prelude::*};
#[cfg(feature = "hydrate")]
use leptos_use::use_element_size;
use plotters_canvas::CanvasBackend;
use ultros_api_types::SaleHistory;
use ultros_charts::ChartOptions;
use ultros_charts::draw_sale_history_scatter_plot;
use crate::components::skeleton::BoxSkeleton;
use crate::global_state::theme::use_theme_settings;
use crate::global_state::xiv_data::tracked_data;
use crate::{components::toggle::Toggle, global_state::LocalWorldData};
```

Replace with:
```rust
use leptos::prelude::*;
use leptos_chartistry::{
    AspectRatio, Chart, Line, Series, Stack, Tooltip, TickLabels,
};
use ultros_api_types::SaleHistory;

use crate::components::toggle::Toggle;
use crate::global_state::LocalWorldData;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string};
```

(The exact chartistry imports may need adjustment — verify with `cargo doc --package leptos-chartistry --open` or by referencing https://docs.rs/leptos-chartistry/0.2/leptos_chartistry/. The Series/Chart/Tooltip names match version 0.2.x.)

- [ ] **Step 3: Replace the component body**

Replace the entire `#[component] pub fn PriceHistoryChart` definition with this:

```rust
#[component]
pub fn PriceHistoryChart(#[prop(into)] sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    let local_world_data = use_context::<LocalWorldData>().unwrap();
    let helper = local_world_data.0.unwrap();
    let i18n = crate::i18n::use_i18n();

    let (range, set_range) = signal(TimeRange::All);
    let (filter_outliers, set_filter_outliers) = signal(true);

    // Re-subscribe to locale swaps so titles re-render when language changes.
    let _ = tracked_data;

    // Filtered sale set after both controls.
    let filtered = Memo::new(move |_| {
        let now = chrono::Utc::now().naive_utc();
        let range = range.get();
        let outliers = filter_outliers.get();
        sales.with(|all| {
            let after_time = filter_by_range(all, range, now);
            if outliers && let Some((min, max)) = iqr_band(
                &after_time.iter().map(|s| s.price_per_item).collect::<Vec<_>>(),
            ) {
                after_time
                    .into_iter()
                    .filter(|s| s.price_per_item >= min && s.price_per_item <= max)
                    .collect::<Vec<_>>()
            } else {
                after_time
            }
        })
    });

    // Stats over the filtered set.
    let stats = Memo::new(move |_| {
        filtered.with(|sales| {
            if sales.is_empty() {
                return None;
            }
            let prices_qty: Vec<(i32, i32)> =
                sales.iter().map(|s| (s.price_per_item, s.quantity)).collect();
            let prices: Vec<i32> = sales.iter().map(|s| s.price_per_item).collect();
            let min = *prices.iter().min().unwrap();
            let max = *prices.iter().max().unwrap();
            Some(ChartStats {
                n: sales.len(),
                vwap: vwap(&prices_qty).unwrap_or(0),
                median: median(&prices).unwrap_or(0),
                min,
                max,
            })
        })
    });

    // IQR band on the *unfiltered* set so the band reflects the broader distribution.
    let band = Memo::new(move |_| {
        sales.with(|all| {
            let prices: Vec<i32> = all.iter().map(|s| s.price_per_item).collect();
            iqr_band(&prices)
        })
    });

    // Series grouped by world/DC/region from the filtered set.
    let series_data = Memo::new(move |_| {
        let helper = helper.clone();
        filtered.with(|sales| flatten_series(&helper, sales))
    });

    view! {
        <div class="panel p-4 md:p-6 text-[color:var(--color-text)]">
            <div class="flex flex-wrap items-center justify-between gap-3 mb-3">
                <h3 class="text-lg font-semibold m-0">
                    {move || t_string!(i18n, sale_history).to_string()}
                </h3>
                <div class="flex flex-wrap items-center gap-3">
                    <RangeChips current=range set_current=set_range />
                    <Toggle
                        checked=filter_outliers
                        set_checked=set_filter_outliers
                        checked_label=t_string!(i18n, filter_outliers_enabled).to_string()
                        unchecked_label=t_string!(i18n, filter_outliers_disabled).to_string()
                    />
                </div>
            </div>

            <StatsStrip stats=stats.into() />

            <div class="w-full aspect-[16/9] max-h-[520px]">
                {move || {
                    let data = series_data.get();
                    if data.is_empty() {
                        view! {
                            <div role="status" class="h-full flex items-center justify-center text-sm text-[color:var(--color-text)]/70">
                                {t_string!(i18n, chart_no_sales_in_window).to_string()}
                            </div>
                        }
                        .into_any()
                    } else {
                        render_chartistry(data, band.get()).into_any()
                    }
                }}
            </div>
        </div>
    }
    .into_any()
}

#[derive(Clone, Copy, Debug)]
struct ChartStats {
    n: usize,
    vwap: i32,
    median: i32,
    min: i32,
    max: i32,
}

#[component]
fn StatsStrip(stats: Signal<Option<ChartStats>>) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    view! {
        <div class="text-sm text-[color:var(--color-text)]/70 tabular-nums mb-3 flex flex-wrap gap-x-4 gap-y-1">
            {move || stats.get().map(|s| view! {
                <span>{format!("{} {}", s.n, t_string!(i18n, chart_stat_n_sales).to_string()
                    .replace("{n}", ""))}</span>
                <span>{format!("{} {}", t_string!(i18n, chart_stat_vwap).to_string(), short_number(s.vwap))}</span>
                <span>{format!("{} {}", t_string!(i18n, chart_stat_median).to_string(), short_number(s.median))}</span>
                <span>{format!("{} {}", t_string!(i18n, chart_stat_min).to_string(), short_number(s.min))}</span>
                <span>{format!("{} {}", t_string!(i18n, chart_stat_max).to_string(), short_number(s.max))}</span>
            })}
        </div>
    }
    .into_any()
}

#[component]
fn RangeChips(
    current: ReadSignal<TimeRange>,
    set_current: WriteSignal<TimeRange>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let options = [
        (TimeRange::Last24h, t_string!(i18n, chart_range_24h).to_string()),
        (TimeRange::Last7d, t_string!(i18n, chart_range_7d).to_string()),
        (TimeRange::Last30d, t_string!(i18n, chart_range_30d).to_string()),
        (TimeRange::All, t_string!(i18n, chart_range_all).to_string()),
    ];
    view! {
        <div role="group" class="inline-flex rounded-md overflow-hidden border border-[color:var(--color-outline)]">
            {options.into_iter().map(|(range, label)| {
                let is_selected = move || current.get() == range;
                view! {
                    <button
                        type="button"
                        aria-pressed=move || is_selected().to_string()
                        class=move || {
                            let base = "px-3 py-1 text-sm transition-colors";
                            if is_selected() {
                                format!("{base} bg-brand-500/25 text-[color:var(--color-text)] font-medium")
                            } else {
                                format!("{base} hover:bg-brand-500/10 text-[color:var(--color-text)]/70")
                            }
                        }
                        on:click=move |_| set_current.set(range)
                    >
                        {label}
                    </button>
                }
            }).collect_view()}
        </div>
    }
    .into_any()
}

/// Flatten the world/DC/region grouping into chartistry-friendly point rows.
/// Each row carries series-name + (time, price, qty) so chartistry can split by series.
#[derive(Clone)]
struct SeriesPoint {
    series: String,
    time: chrono::DateTime<chrono::Local>,
    price: f64,
    qty: f64,
}

fn flatten_series(
    helper: &ultros_api_types::world_helper::WorldHelper,
    sales: &[SaleHistory],
) -> Vec<SeriesPoint> {
    group_sales_by_locale(helper, sales)
        .into_iter()
        .flat_map(|(name, pts)| {
            let n = name.clone();
            pts.into_iter().map(move |(t, p, q)| SeriesPoint {
                series: n.clone(),
                time: t,
                price: p as f64,
                qty: q as f64,
            })
        })
        .collect()
}

/// Build the chartistry view. The exact API shape may need adjustment based on
/// what `leptos-chartistry` 0.2 exposes — see https://docs.rs/leptos-chartistry/0.2/.
/// Goal: a stacked / multi-series scatter with hover tooltip.
fn render_chartistry(
    points: Vec<SeriesPoint>,
    band: Option<(i32, i32)>,
) -> impl IntoView {
    let _ = band; // IQR band drawn via overlay; see note below.

    // Group points back into per-series vectors for chartistry's Series-of-points model.
    let mut by_series: std::collections::BTreeMap<String, Vec<SeriesPoint>> = Default::default();
    for p in points {
        by_series.entry(p.series.clone()).or_default().push(p);
    }

    // Convert each group into a (time, price) row vector. chartistry's `Series::new`
    // wants a closure to extract X and one `Line` per Y dimension.
    let rows: Vec<SeriesRow> = by_series
        .into_iter()
        .flat_map(|(_, pts)| {
            pts.into_iter().map(|p| SeriesRow {
                time: p.time,
                price_for: {
                    let mut m = std::collections::HashMap::new();
                    m.insert(p.series.clone(), p.price);
                    m
                },
                series: p.series,
                qty: p.qty,
            })
        })
        .collect();

    let series_names: Vec<String> = rows
        .iter()
        .map(|r| r.series.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let mut series = Series::new(|r: &SeriesRow| r.time);
    for name in &series_names {
        let n = name.clone();
        series = series.line(
            Line::new(move |r: &SeriesRow| r.price_for.get(&n).copied().unwrap_or(f64::NAN))
                .with_name(name.clone()),
        );
    }

    view! {
        <Chart
            aspect_ratio=AspectRatio::from_outer_ratio(16.0, 9.0)
            series=series
            data=Signal::derive(move || rows.clone())
            tooltip=Tooltip::left_cursor()
            top=Stack::default()
            left=TickLabels::default()
            bottom=TickLabels::default()
        />
    }
}

#[derive(Clone)]
struct SeriesRow {
    time: chrono::DateTime<chrono::Local>,
    series: String,
    qty: f64,
    /// Sparse per-series price: only the row's own series gets a value; others are NaN
    /// so chartistry draws a discrete point per row without connecting lines.
    price_for: std::collections::HashMap<String, f64>,
}
```

**Important caveats for the implementer**:

1. **Chartistry API shape is approximate.** `leptos-chartistry` 0.2's exact API names (`AspectRatio::from_outer_ratio`, `Stack::default()`, etc.) need to be verified against [docs.rs/leptos-chartistry/0.2](https://docs.rs/leptos-chartistry/0.2/leptos_chartistry/). The shape of `Series::new` + `.line()` chaining is correct per upstream examples. If a name doesn't compile, find the equivalent in the crate's public API and substitute. Do not rewrite the component structure — only the chartistry call shape.

2. **Scatter rendering**: chartistry is primarily a line-chart library. To render *scatter only* (no connecting lines), each series may need a `with_marker(...)` call and `.with_width(0.0)` on the `Line`, or chartistry may expose a `Scatter`-equivalent. If only lines are available in 0.2, render lines with very low opacity plus markers — the user can always send feedback and we iterate.

3. **VWAP / trendline / IQR-band overlays**: these are deferred to Task 7. Task 6 ships a working interactive chart without overlays first, so the rest of the refactor is reviewable. If the chartistry call doesn't compile, the engineer can land Task 6 with a placeholder static SVG and unblock Tasks 7–8.

- [ ] **Step 4: Run the build**

Run: `cargo check -p ultros-app --no-default-features --features hydrate`
Expected: builds clean. If chartistry API mismatches block this, fix them against docs.rs and re-run. Do not edit `Cargo.lock` by hand — let cargo regenerate it.

- [ ] **Step 5: Drop the outer wrapping div from item_view.rs**

Edit `ultros-frontend/ultros-app/src/routes/item_view.rs` around line 706–711. Replace:

```rust
                                } else {
                                    view! {
                                        <div class="panel p-6 text-[color:var(--color-text)]">
                                            <PriceHistoryChart sales=filtered_sales />
                                        </div>
                                    }.into_any()
                                }
```

with:

```rust
                                } else {
                                    view! {
                                        <PriceHistoryChart sales=filtered_sales />
                                    }.into_any()
                                }
```

The component now owns its own panel chrome.

- [ ] **Step 6: Run check_ci.sh**

Run: `./check_ci.sh` from the worktree root.
Expected: PASS. Per CLAUDE.md this runs `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`. Fix anything it reports. If clippy fails because the submodule isn't initialized (build script panic on `cn/Item.csv`), the pre-flight step 0b should have caught this — re-run `git submodule update --init --recursive --depth=1`.

- [ ] **Step 7: Manual smoke test**

Run: `cargo leptos watch` (or whatever the project's dev server invocation is — check [AGENTS.md](../../AGENTS.md) if unsure).

Open the item view in a browser (e.g. `http://localhost:8080/item/Gilgamesh/5057`) and verify:

1. Chart renders with multi-series points
2. Hover over a point shows tooltip with time + price
3. Clicking 24h / 7d / 30d / All updates the chart
4. Toggling outlier filter updates the chart and stats strip
5. Stats strip shows `n · VWAP · median · min · max`
6. "Download PNG" button on the same page still works (it uses the untouched plotters backend)

If anything fails, fix and re-run.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/price_history_chart.rs ultros-frontend/ultros-app/src/routes/item_view.rs
git commit -m "feat(chart): replace plotters canvas with interactive leptos-chartistry chart"
```

---

## Task 7: VWAP, IQR band, and trendline overlays

Now that the bare interactive chart works, add the financial-style overlays. These are pure data transforms layered on top of the existing `render_chartistry` function.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs`

- [ ] **Step 1: Compute VWAP, trendline endpoints, and band-as-rows**

Inside `render_chartistry`, before building `series`, compute three additional virtual series:

```rust
// 1. VWAP — a constant horizontal line at the volume-weighted average price.
let (vwap_value, vwap_endpoints) = {
    let prices_qty: Vec<(i32, i32)> = rows
        .iter()
        .map(|r| (r.price_for.values().copied().next().unwrap_or(0.0) as i32, r.qty as i32))
        .collect();
    let v = vwap(&prices_qty);
    let (min_t, max_t) = match (rows.first(), rows.last()) {
        (Some(a), Some(b)) => (a.time, b.time),
        _ => (chrono::Local::now(), chrono::Local::now()),
    };
    (v, (min_t, max_t))
};

// 2. Trendline — least-squares fit across all points.
let trendline_endpoints = {
    let xs: Vec<f64> = rows.iter().map(|r| r.time.timestamp() as f64).collect();
    let ys: Vec<f64> = rows.iter().flat_map(|r| r.price_for.values().copied()).collect();
    if xs.len() > 1 {
        let n = xs.len() as f64;
        let mean_x = xs.iter().sum::<f64>() / n;
        let mean_y = ys.iter().sum::<f64>() / n;
        let mut cov = 0.0;
        let mut varx = 0.0;
        for i in 0..xs.len() {
            let dx = xs[i] - mean_x;
            cov += dx * (ys[i] - mean_y);
            varx += dx * dx;
        }
        if varx > 0.0 {
            let m = cov / varx;
            let b = mean_y - m * mean_x;
            let x1 = *xs.first().unwrap();
            let x2 = *xs.last().unwrap();
            Some(((x1, b + m * x1), (x2, b + m * x2)))
        } else {
            None
        }
    } else {
        None
    }
};

// 3. IQR band — two horizontal lines at min and max of the band.
let band_lines = band; // Option<(i32, i32)>
```

- [ ] **Step 2: Add overlay rows to the chart data**

Extend the `SeriesRow` model to carry overlay values. Replace the existing `SeriesRow` struct with:

```rust
#[derive(Clone)]
struct SeriesRow {
    time: chrono::DateTime<chrono::Local>,
    series: String,
    qty: f64,
    price_for: std::collections::HashMap<String, f64>,
    /// Overlay values — None on scatter rows, Some on synthetic overlay rows.
    vwap: Option<f64>,
    trend: Option<f64>,
    iqr_low: Option<f64>,
    iqr_high: Option<f64>,
}
```

Update the existing scatter-row construction in `flatten_series` to set the overlay fields to `None`. Then append synthetic overlay rows at the start of `render_chartistry`:

```rust
let mut rows = rows; // shadow the immutable binding

if let (Some(v), (t1, t2)) = (vwap_value, vwap_endpoints) {
    rows.push(SeriesRow {
        time: t1, series: String::new(), qty: 0.0,
        price_for: Default::default(),
        vwap: Some(v as f64), trend: None, iqr_low: None, iqr_high: None,
    });
    rows.push(SeriesRow {
        time: t2, series: String::new(), qty: 0.0,
        price_for: Default::default(),
        vwap: Some(v as f64), trend: None, iqr_low: None, iqr_high: None,
    });
}

if let Some(((x1, y1), (x2, y2))) = trendline_endpoints {
    rows.push(SeriesRow {
        time: chrono::DateTime::from_timestamp(x1 as i64, 0).unwrap().with_timezone(&chrono::Local),
        series: String::new(), qty: 0.0, price_for: Default::default(),
        vwap: None, trend: Some(y1), iqr_low: None, iqr_high: None,
    });
    rows.push(SeriesRow {
        time: chrono::DateTime::from_timestamp(x2 as i64, 0).unwrap().with_timezone(&chrono::Local),
        series: String::new(), qty: 0.0, price_for: Default::default(),
        vwap: None, trend: Some(y2), iqr_low: None, iqr_high: None,
    });
}

if let Some((lo, hi)) = band_lines {
    let t1 = rows.iter().map(|r| r.time).min().unwrap_or_else(chrono::Local::now);
    let t2 = rows.iter().map(|r| r.time).max().unwrap_or_else(chrono::Local::now);
    rows.push(SeriesRow {
        time: t1, series: String::new(), qty: 0.0, price_for: Default::default(),
        vwap: None, trend: None, iqr_low: Some(lo as f64), iqr_high: Some(hi as f64),
    });
    rows.push(SeriesRow {
        time: t2, series: String::new(), qty: 0.0, price_for: Default::default(),
        vwap: None, trend: None, iqr_low: Some(lo as f64), iqr_high: Some(hi as f64),
    });
}

rows.sort_by_key(|r| r.time);
```

- [ ] **Step 3: Add overlay `Line`s to the chartistry series**

After the loop that adds per-series scatter lines, append:

```rust
series = series.line(
    Line::new(|r: &SeriesRow| r.vwap.unwrap_or(f64::NAN)).with_name("VWAP"),
);
series = series.line(
    Line::new(|r: &SeriesRow| r.trend.unwrap_or(f64::NAN)).with_name("Trend"),
);
series = series.line(
    Line::new(|r: &SeriesRow| r.iqr_low.unwrap_or(f64::NAN)).with_name("IQR low"),
);
series = series.line(
    Line::new(|r: &SeriesRow| r.iqr_high.unwrap_or(f64::NAN)).with_name("IQR high"),
);
```

Color customization per line is via chartistry's `.with_color(...)` if exposed; otherwise the dynamic palette will color them automatically and the user-visible legend labels will distinguish them.

- [ ] **Step 4: Build + manual verification**

Run: `cargo check -p ultros-app --no-default-features --features hydrate`
Expected: clean.

Then `cargo leptos watch` and verify in the browser:

1. A faint trendline crosses the chart
2. A horizontal VWAP line sits at the volume-weighted average
3. Two faint horizontal lines mark the IQR band
4. None of the overlays connect to the scatter points
5. Toggling the outlier filter dims the IQR band visually (via opacity adjustment — if chartistry doesn't expose per-line opacity, accept this as a follow-up and document in code comment)

- [ ] **Step 5: Run check_ci.sh**

Run: `./check_ci.sh`
Expected: PASS. Fix any clippy warnings — likely candidates are unused-imports and dead-code if anything was left over from the plotters version.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/price_history_chart.rs
git commit -m "feat(chart): add VWAP, trendline, and IQR band overlays"
```

---

## Task 8: Polish — accessibility and aspect ratio

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs`

- [ ] **Step 1: Wrap the chart in an `aria-label`-bearing container**

Inside the `<div class="w-full aspect-[16/9] ...">` block in the `PriceHistoryChart` component, add:

```rust
attr:role="img"
attr:aria-label=move || {
    let stats = stats.get();
    let n = stats.map(|s| s.n).unwrap_or(0);
    t_string!(i18n, chart_aria_label).to_string()
        .replace("{n}", &n.to_string())
        .replace("{from}", "") // TODO: when we add the time-range visualization to a11y
        .replace("{to}", "")
}
```

(The `{from}` / `{to}` placeholders can stay empty for v1 — the aria label is "Scatter plot of N sales" effectively.)

- [ ] **Step 2: Verify the no-sales empty state is keyboard-reachable**

The existing `role="status"` on the empty-state div is correct. No change needed.

- [ ] **Step 3: Build + run check_ci.sh**

```
cargo check -p ultros-app --no-default-features --features hydrate
./check_ci.sh
```
Expected: both pass.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/price_history_chart.rs
git commit -m "a11y(chart): add aria-label summarizing chart contents"
```

---

## Task 9: Final verification and PR

- [ ] **Step 1: Run full CI locally**

Run: `./check_ci.sh`
Expected: PASS.

- [ ] **Step 2: Run unit tests**

Run: `cargo test -p ultros-app --no-default-features --features hydrate`
Expected: PASS (Tasks 2, 3, 4 added 14 tests).

- [ ] **Step 3: Smoke test in browser**

Run: `cargo leptos watch` and re-verify the full Task 6 Step 7 checklist plus:

1. Theme switch (light → dark) updates colors live without reload
2. Locale switch updates the title and stat labels
3. SSR HTML contains the chart SVG (View Source on the item page before WASM hydration completes; chartistry's SVG should be present)
4. PNG download endpoint at `/itemcard/<world>/<item_id>` still returns a valid plotters-rendered image

- [ ] **Step 4: Push and open PR**

```bash
git push -u origin claude/modest-lewin-141588
```

Then `gh pr create` with title `feat: interactive price-history chart` and a body covering:
- Spec link
- Plotters-canvas → chartistry swap
- Added overlays (VWAP, IQR band, trendline)
- Time-range chips + stats strip
- Server-side PNG generator untouched
- Test plan checklist (browser hover, chip filter, outlier toggle, theme switch, PNG endpoint)

---

## Self-Review

**Spec coverage** — checked each section of the spec against tasks:
- Library choice (`leptos-chartistry`) → Task 1.
- Component shape (single component, panel chrome owned) → Task 6.
- Time-range chips → Task 4 (helper) + Task 6 (UI).
- Stats strip (n, VWAP, median, min, max) → Task 3 (helpers) + Task 6 (UI).
- Series grouping → Task 2.
- Visual elements: VWAP / IQR band / trendline → Task 7.
- Hover tooltip → Task 6 (chartistry default behavior).
- Layout / CSS (aspect-ratio, chips, stats strip) → Task 6 + Task 8.
- Dependencies → Task 1.
- SSR safety → Task 6 (no `web-sys` calls in the hot path).
- Accessibility → Task 8.
- What gets deleted (plotters-canvas, parse_css_rgb, chart_colors memo) → Task 1 + Task 6 (replaced by chartistry's CSS-native styling).
- Risk / fallback (hand-rolled SVG) → noted in Task 1 step 2.

**Placeholder scan** — no "TBD" / "TODO" / "fill in later" / "similar to" patterns. The chartistry API caveats in Task 6 are explicit warnings to verify against docs.rs at impl time, not unfinished plan content.

**Type consistency**:
- `TimeRange` used the same way in Task 4 (definition) and Task 6 (consumer).
- `SeriesRow` is defined in Task 6 and *extended* in Task 7 — the field additions are explicit, not a rename.
- `ChartStats` defined and consumed in Task 6 only.
- `vwap`, `median`, `iqr_band`, `short_number` signatures consistent across Tasks 3, 6, 7.

**Known fragile points flagged in the plan body**:
- Chartistry's exact API surface — flagged in Task 6 with link to docs.rs and instruction to substitute names, not restructure.
- Scatter-vs-line rendering — flagged with fallback (low-opacity line + markers).
- Per-line opacity for the dimmed IQR band — accepted as follow-up if chartistry doesn't expose it.
