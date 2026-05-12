# Analyzer Data Integrity & Guidance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Strip the 1-gil sniper rows from the default Flip Finder view by replacing the analyzer's profit-estimate math with sanity-clamped, median-based stats, then re-tune presets and filters so the landing experience guides users to realistic flips.

**Architecture:** All changes are localized to a single Leptos route file ([ultros-frontend/ultros-app/src/routes/analyzer.rs](../../ultros-frontend/ultros-app/src/routes/analyzer.rs)) plus the English locale JSON. No API, DB, or shared-types changes. The work is broken into pure-function logic changes (TDD via Rust unit tests) followed by Leptos query-param/filter-card wiring.

**Tech Stack:** Rust, Leptos (SSR/hydrate), `leptos_i18n`, `humantime`, `chrono`. Tests are stock `#[cfg(test)] mod tests` blocks inside the same file.

---

## File Structure

- **Modify** [ultros-frontend/ultros-app/src/routes/analyzer.rs](../../ultros-frontend/ultros-app/src/routes/analyzer.rs)
  - `SaleSummary` gains `median_price: i32` and `days_since_last_sale: Option<Duration>`.
  - `compute_summary` loses the `hq_data` parameter, gains a sniper-clamp, computes a median.
  - `ProfitTable::new` gains a troll-listing guard and consumes the row's median instead of min.
  - `AnalyzerTable` gains two new query signals (`min-buy`, `last-sold`), two filter cards, chips, "Clear all" wiring, and a new "Last sold" table column.
  - Preset filter buttons re-tuned; one new "Realistic flips" preset added as the leading CTA.
  - A new `#[cfg(test)] mod tests` block at the bottom covers the pure-function changes.
- **Modify** [ultros-frontend/ultros-app/locales/en.json](../../ultros-frontend/ultros-app/locales/en.json)
  - Six new keys for the new filter cards, chips, column header, and preset label.

No new files. The other six locale JSONs (`cn`, `de`, `fr`, `ja`, `ko`, `tc`) already only populate ~2 analyzer keys each — `leptos_i18n` falls back to English for missing keys, so we don't touch them. (If a clippy/build error surfaces a missing key, copy the new English values into each file as a follow-up.)

---

## Task 1: Add a median + sniper-clamp to `SaleSummary`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs:36-147`

The goal of this task is two pure-function changes inside `compute_summary`:
1. Drop any sale priced below `0.1 × raw_median` before computing the final summary (sniper guard).
2. Add a `median_price: i32` field computed from the (clamped) prices.

We also drop the HQ→NQ contamination here: `hq_data` parameter is removed entirely.

- [ ] **Step 1: Write the failing tests**

Append to `ultros-frontend/ultros-app/src/routes/analyzer.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::recent_sales::{SaleData, Sales};

    fn sale(price: i32, days_ago: i64) -> Sales {
        let date = Utc::now()
            .naive_utc()
            .checked_sub_signed(Duration::days(days_ago))
            .unwrap();
        Sales { price_per_unit: price, sale_date: date }
    }

    fn sales_row(item_id: i32, hq: bool, prices_and_days: &[(i32, i64)]) -> SaleData {
        SaleData {
            item_id,
            hq,
            sales: prices_and_days.iter().map(|(p, d)| sale(*p, *d)).collect(),
        }
    }

    #[test]
    fn median_price_is_middle_of_clamped_sales() {
        let row = sales_row(1, false, &[(100, 0), (110, 1), (120, 2), (130, 3), (140, 4), (150, 5)]);
        let summary = compute_summary(row, false);
        // Six even-length sample: median = (third + fourth) / 2 = (120 + 130) / 2 = 125
        assert_eq!(summary.median_price, 125);
    }

    #[test]
    fn sniper_sale_below_10pct_of_median_is_dropped() {
        // Raw median of [1, 100, 110, 120, 130, 140] sorted = (110+120)/2 = 115.
        // The "1" is well below 10% of 115 (=11), so it's dropped.
        let row = sales_row(2, false, &[(1, 0), (100, 1), (110, 2), (120, 3), (130, 4), (140, 5)]);
        let summary = compute_summary(row, false);
        // Median of remaining [100, 110, 120, 130, 140] = 120.
        assert_eq!(summary.median_price, 120);
        // min_price should also reflect the clamp, not the sniper.
        assert_eq!(summary.min_price, 100);
    }

    #[test]
    fn hq_prices_do_not_contaminate_nq_summary() {
        // An NQ row with normal prices. compute_summary no longer takes HQ context.
        let row = sales_row(3, false, &[(500, 0), (510, 1), (520, 2), (530, 3), (540, 4), (550, 5)]);
        let summary = compute_summary(row, false);
        assert_eq!(summary.min_price, 500);
        assert_eq!(summary.median_price, 525);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:
```
cargo test -p ultros-app --lib routes::analyzer::tests
```
Expected: all three tests fail. The first two fail because `median_price` doesn't exist on `SaleSummary`. The third fails because `compute_summary` currently takes three args (`sale, hq_data, filter_outliers`).

- [ ] **Step 3: Update `SaleSummary` and `compute_summary`**

Replace the `SaleSummary` struct and `compute_summary` function at [analyzer.rs:36-147](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:36) with:

```rust
/// Computed sale stats
#[derive(Hash, Clone, Debug, PartialEq)]
struct SaleSummary {
    item_id: i32,
    hq: bool,
    /// this value is limited by the summary returned by the API
    num_sold: usize,
    /// Represents the average time between sales within the `num_sold`
    avg_sale_duration: Option<Duration>,
    /// Time since the most-recent sale. `None` if no sales.
    days_since_last_sale: Option<Duration>,
    max_price: i32,
    avg_price: i32,
    /// Robust mid-point of the clamped sales — used as the realistic seller estimate.
    median_price: i32,
    /// Floor of the clamped sales — worst-case undercut.
    min_price: i32,
}

/// Sniper-clamp threshold: drop any sale priced below this fraction of the raw median.
const SNIPER_FRACTION: f64 = 0.1;

fn median_i32(sorted: &[i32]) -> i32 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        ((sorted[n / 2 - 1] as i64 + sorted[n / 2] as i64) / 2) as i32
    }
}

fn compute_summary(sale: SaleData, filter_outliers: bool) -> SaleSummary {
    let now = Utc::now().naive_utc();
    let SaleData { item_id, hq, sales } = sale;

    if sales.is_empty() {
        return SaleSummary {
            item_id,
            hq,
            num_sold: 0,
            avg_sale_duration: None,
            days_since_last_sale: None,
            max_price: 0,
            avg_price: 0,
            median_price: 0,
            min_price: 0,
        };
    }

    // 1. Raw-median pass for the sniper threshold.
    let mut raw: Vec<i32> = sales.iter().map(|s| s.price_per_unit).collect();
    raw.sort_unstable();
    let raw_median = median_i32(&raw);
    let floor = (raw_median as f64 * SNIPER_FRACTION) as i32;

    // 2. Build the clamped vector. If the clamp would remove everything, keep the raw set.
    let mut clamped: Vec<i32> = raw.iter().copied().filter(|p| *p >= floor).collect();
    if clamped.is_empty() {
        clamped = raw.clone();
    }
    let median_price = median_i32(&clamped);
    let min_price = *clamped.first().unwrap_or(&0);
    let max_price = *clamped.last().unwrap_or(&0);

    // 3. Average price respects the existing IQR filter-outliers toggle.
    let avg_price = if filter_outliers {
        let mut prices = clamped.clone();
        let filtered = filter_outliers_iqr_in_place(&mut prices);
        if filtered.is_empty() {
            0
        } else {
            (filtered.iter().map(|&p| p as i64).sum::<i64>() / filtered.len() as i64) as i32
        }
    } else {
        (clamped.iter().map(|&p| p as i64).sum::<i64>() / clamped.len() as i64) as i32
    };

    // 4. Velocity. Newest first in the API's response.
    let newest = sales.first().map(|s| s.sale_date);
    let oldest = sales.last().map(|s| s.sale_date);
    let avg_sale_duration = oldest.map(|last| {
        let ms = (last - now).num_milliseconds().abs() / sales.len() as i64;
        Duration::milliseconds(ms)
    });
    let days_since_last_sale =
        newest.map(|n| Duration::milliseconds((now - n).num_milliseconds().max(0)));

    SaleSummary {
        item_id,
        hq,
        num_sold: sales.len(),
        avg_sale_duration,
        days_since_last_sale,
        max_price,
        avg_price,
        median_price,
        min_price,
    }
}
```

Note this signature change deliberately breaks `ProfitTable::new`. Task 2 fixes that.

- [ ] **Step 4: Run the new tests to verify they pass**

Run:
```
cargo test -p ultros-app --lib routes::analyzer::tests
```
Expected: the three tests pass. The whole crate will still fail to compile (`ProfitTable::new` calls `compute_summary` with the old signature) — that's fine for now.

- [ ] **Step 5: Do not commit yet**

The crate doesn't build. Commit happens at the end of Task 2 once the call site is fixed.

---

## Task 2: Wire `ProfitTable::new` to the new `compute_summary` + add troll-listing guard

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs:174-244`

Three sub-changes:
1. Drop the `hq_sales` map and the `hq_data` argument at the call site.
2. Use `summary.median_price` (not `min_price`) as the historical anchor for `estimated_sale_price`.
3. Skip any world-floor entry that's ≥ 50× the row's median (troll guard).

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block:

```rust
#[test]
fn troll_world_floor_does_not_inflate_estimate() {
    use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};
    use ultros_api_types::recent_sales::RecentSales;

    let sales = RecentSales {
        sales: vec![sales_row(100, false, &[(1000, 0), (1000, 1), (1100, 2), (1000, 3), (1050, 4), (1000, 5)])],
    };
    // Region cheapest = a troll 999,999,999 listing on a foreign world.
    let region = CheapestListings {
        cheapest_listings: vec![CheapestListingItem {
            item_id: 100,
            hq: false,
            cheapest_price: 999_999_999,
            world_id: 42,
        }],
    };
    // Our own world has a sane cheapest at 1100.
    let world = CheapestListings {
        cheapest_listings: vec![CheapestListingItem {
            item_id: 100,
            hq: false,
            cheapest_price: 1100,
            world_id: 1,
        }],
    };

    let table = ProfitTable::new(sales, region, world, vec![], false);
    assert_eq!(table.0.len(), 1);
    let row = &table.0[0];
    // The troll 999,999,999 floor must NOT be used — the row should fall through to median (=1025)
    // capped against the local world floor (1100), so the estimated sale price is 1025.
    assert_eq!(row.sale_summary.median_price, 1025);
    assert_eq!(row.estimated_sale_price, 1025);
}

#[test]
fn estimated_sale_price_uses_median_not_min() {
    use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};
    use ultros_api_types::recent_sales::RecentSales;

    let sales = RecentSales {
        sales: vec![sales_row(
            200,
            false,
            &[(800, 0), (1000, 1), (1000, 2), (1000, 3), (1000, 4), (1200, 5)],
        )],
    };
    // Region floor is below median (a sane off-world deal).
    let region = CheapestListings {
        cheapest_listings: vec![CheapestListingItem {
            item_id: 200,
            hq: false,
            cheapest_price: 700,
            world_id: 42,
        }],
    };
    // Local world floor is well above the median — the estimate should pin to median (=1000),
    // not min (=800) and not the world floor (=5000).
    let world = CheapestListings {
        cheapest_listings: vec![CheapestListingItem {
            item_id: 200,
            hq: false,
            cheapest_price: 5000,
            world_id: 1,
        }],
    };

    let table = ProfitTable::new(sales, region, world, vec![], false);
    assert_eq!(table.0.len(), 1);
    let row = &table.0[0];
    assert_eq!(row.sale_summary.median_price, 1000);
    assert_eq!(row.estimated_sale_price, 1000);
}
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run:
```
cargo test -p ultros-app --lib routes::analyzer::tests
```
Expected: compile error because `ProfitTable::new` still calls `compute_summary` with the old three-arg signature.

- [ ] **Step 3: Replace `ProfitTable::new`**

Replace the body of `impl ProfitTable` at [analyzer.rs:174-244](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:174) with:

```rust
/// Listings whose price is at least this multiple of the row's median sale are treated as troll
/// listings and ignored when picking the world floor.
const TROLL_MULTIPLE: i64 = 50;

fn is_troll_listing(price: i32, median: i32) -> bool {
    median > 0 && (price as i64) > (median as i64).saturating_mul(TROLL_MULTIPLE)
}

impl ProfitTable {
    fn new(
        sales: RecentSales,
        global_cheapest_listings: CheapestListings,
        world_cheapest_listings: CheapestListings,
        cross_region: Vec<CheapestListings>,
        filter_outliers: bool,
    ) -> Self {
        let mut region_cheapest = listings_to_map(global_cheapest_listings);
        let world_cheapest = listings_to_map(world_cheapest_listings);

        for cross in cross_region.into_iter().map(listings_to_map) {
            for (key, (new_price, world_id)) in cross {
                match region_cheapest.entry(key) {
                    Entry::Occupied(mut entry) => {
                        let (current_price, _) = entry.get();
                        if *current_price > new_price {
                            entry.insert((new_price, world_id));
                        }
                    }
                    Entry::Vacant(e) => {
                        e.insert((new_price, world_id));
                    }
                }
            }
        }

        let table = sales
            .sales
            .into_iter()
            .flat_map(|sale| {
                let item_id = sale.item_id;
                let hq = sale.hq;
                let key = ProfitKey { item_id, hq };
                let (raw_region_price, region_world_id) = *region_cheapest.get(&key)?;
                let summary = compute_summary(sale, filter_outliers);

                // Troll-listing guard: if the region floor is implausibly high vs the median,
                // drop the row entirely — the displayed "deal" would be fictional.
                if is_troll_listing(raw_region_price, summary.median_price) {
                    return None;
                }

                // Same guard on the local world floor — if it's a troll, ignore it and fall
                // through to the median as the estimate.
                let world_floor = world_cheapest.get(&key).and_then(|(price, _)| {
                    if is_troll_listing(*price, summary.median_price) {
                        None
                    } else {
                        Some(*price)
                    }
                });

                let estimated_sale_price = match world_floor {
                    Some(floor) => summary.median_price.min(floor),
                    None => summary.median_price,
                };

                Some(ProfitData {
                    estimated_sale_price,
                    sale_summary: summary,
                    cheapest_world_id: region_world_id,
                    cheapest_price: raw_region_price,
                })
            })
            .map(Arc::new)
            .collect();

        ProfitTable(table)
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run:
```
cargo test -p ultros-app --lib routes::analyzer::tests
```
Expected: all five tests pass (3 from Task 1 + 2 new). The crate should build cleanly now.

- [ ] **Step 5: Run the workspace-wide CI check**

Run:
```
./check_ci.sh
```
Expected: fmt-check passes, clippy passes. If clippy complains, fix the underlying issue.

- [ ] **Step 6: Commit**

```
git add ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "$(cat <<'EOF'
fix(analyzer): median-based sell estimate + sniper/troll clamps

Drops HQ price contamination of NQ rows, switches the historical sale
anchor from min-of-six to median-of-clamped-sales, and ignores world
listings priced 50x above the median (troll guard) when computing
estimated_sale_price. Adds days_since_last_sale to SaleSummary in
preparation for the "Last sold" column.

Unit tests cover the median, sniper, troll, and HQ-isolation cases.
EOF
)"
```

---

## Task 3: Add the "Last sold" table column

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs:780-1032`
- Modify: `ultros-frontend/ultros-app/locales/en.json`

Surfaces the new `days_since_last_sale` field as its own column, after "Avg Sale Time".

- [ ] **Step 1: Add the locale string**

Open [ultros-frontend/ultros-app/locales/en.json](../../ultros-frontend/ultros-app/locales/en.json) and locate the `analyzer_col_avg_sale_time` line (~line 386). Add the following line directly after it:

```json
    "analyzer_col_last_sold": "Last Sold",
```

Also add (we use it in step 3 of the cell renderer):

```json
    "analyzer_last_sold_never": "—",
```

- [ ] **Step 2: Add the column header**

In `AnalyzerTable`'s header `view!` at [analyzer.rs:879-883](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:879), add a new `columnheader` right after the existing "Avg Sale Time" header:

```rust
<div role="columnheader" class="w-30 p-4 hidden md:block">
    {t!(i18n, analyzer_col_last_sold)}
</div>
```

- [ ] **Step 3: Add the cell renderer**

In the per-row `view!` at [analyzer.rs:1008-1027](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:1008), add a new cell directly after the "Avg Sale Time" cell:

```rust
<div role="cell" class="px-4 py-2 w-30 truncate hidden md:block flex items-center">
    {data.inner
        .sale_summary
        .days_since_last_sale
        .and_then(|d| d.to_std().ok())
        .map(|d| {
            let secs = d.as_secs();
            let days = secs / 86_400;
            let hours = (secs % 86_400) / 3_600;
            if days > 0 { format!("{}d ago", days) }
            else if hours > 0 { format!("{}h ago", hours) }
            else { "just now".to_string() }
        })
        .unwrap_or_else(|| t_string!(i18n, analyzer_last_sold_never).to_string())}
</div>
```

- [ ] **Step 4: Build and run E2E smoke**

Run:
```
./check_ci.sh
```
Expected: fmt and clippy pass.

- [ ] **Step 5: Commit**

```
git add ultros-frontend/ultros-app/src/routes/analyzer.rs ultros-frontend/ultros-app/locales/en.json
git commit -m "feat(analyzer): show days since last sale per row"
```

---

## Task 4: Add the `min-buy` (Minimum Buy Price) filter

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs`
- Modify: `ultros-frontend/ultros-app/locales/en.json`

This is the single most important filter — it's what removes the 1-gil rows from any sort that surfaces them.

- [ ] **Step 1: Add the locale strings**

Append to the analyzer section of [ultros-frontend/ultros-app/locales/en.json](../../ultros-frontend/ultros-app/locales/en.json):

```json
    "analyzer_minimum_buy_price": "Minimum Buy Price",
    "analyzer_minimum_buy_price_desc": "Hide rows where the buy-price is below this floor — filters out price-war and sniper listings",
    "analyzer_min_buy_gte": "Buy ≥ ",
```

- [ ] **Step 2: Add the query signal**

In `AnalyzerTable` near the existing `max_purchase_price` signal at [analyzer.rs:288](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:288), add:

```rust
let (min_buy_price, set_min_buy_price) = query_signal::<i32>("min-buy");
```

- [ ] **Step 3: Add a `.filter` to `sorted_data`**

Inside the `Memo::new(move |_| { ... })` for `sorted_data`, add another `.filter` chained alongside the existing `max_purchase_price` filter at [analyzer.rs:379-383](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:379):

```rust
.filter(move |data| {
    min_buy_price()
        .map(|min| data.inner.cheapest_price >= min)
        .unwrap_or(true)
})
```

- [ ] **Step 4: Add the filter card**

In the FilterCard grid, after the "Maximum Budget" card at [analyzer.rs:582-611](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:582), add:

```rust
<FilterCard
    title=t_string!(i18n, analyzer_minimum_buy_price).to_string()
    description=t_string!(i18n, analyzer_minimum_buy_price_desc).to_string()
>
    <div class="flex flex-col gap-2">
        <div class="text-brand-300">
            {move || {
                min_buy_price()
                    .map(|p| Either::Left(view! { <Gil amount=p /> }))
                    .unwrap_or(Either::Right("---"))
            }}
        </div>
        <input
            class="input"
            min=0
            step=1000
            placeholder="e.g. 5000"
            type="number"
            prop:value=min_buy_price
            on:input=move |input| {
                let value = event_target_value(&input);
                if let Ok(p) = value.parse::<i32>() {
                    set_min_buy_price(Some(p));
                } else if value.is_empty() {
                    set_min_buy_price(None);
                }
            }
        />
    </div>
</FilterCard>
```

- [ ] **Step 5: Add the chip and clear-all wiring**

In the chips block at [analyzer.rs:653-755](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:653), add another `if let Some(...)` block alongside the existing `max_purchase_price` chip:

```rust
if let Some(p) = min_buy_price() {
    chips.push(view! {
        <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
            {t!(i18n, analyzer_min_buy_gte)} <Gil amount=p />
            <button aria-label="Remove filter" class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_min_buy_price(None)>
                <Icon icon=icondata::MdiClose />
            </button>
        </span>
    }.into_any());
}
```

In the "Clear all" button at [analyzer.rs:757-769](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:757), add `set_min_buy_price(None);` alongside the other setters.

- [ ] **Step 6: Build and verify**

Run:
```
./check_ci.sh
```
Expected: fmt and clippy pass.

- [ ] **Step 7: Commit**

```
git add ultros-frontend/ultros-app/src/routes/analyzer.rs ultros-frontend/ultros-app/locales/en.json
git commit -m "feat(analyzer): minimum buy-price filter (?min-buy=)"
```

---

## Task 5: Add the `last-sold` (Last Sold Within) filter

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs`
- Modify: `ultros-frontend/ultros-app/locales/en.json`

Mirror the existing `next-sale` filter pattern, but applied to `days_since_last_sale` rather than `avg_sale_duration`.

- [ ] **Step 1: Add the locale strings**

Append to the analyzer section of `en.json`:

```json
    "analyzer_last_sold_within": "Last Sold Within",
    "analyzer_last_sold_within_desc": "Hide rows whose most recent sale is older than this (e.g. 7d, 1d 12h)",
    "analyzer_last_sold_lte": "Last sold ≤ ",
```

- [ ] **Step 2: Add the query signal + memo**

In `AnalyzerTable` near the existing `max_predicted_time` signal at [analyzer.rs:282](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:282), add:

```rust
let (last_sold_within, set_last_sold_within) = query_signal::<String>("last-sold");
let last_sold_duration =
    Memo::new(move |_| last_sold_within().and_then(|d| parse_duration(d.as_str()).ok()));
let last_sold_string = Memo::new(move |_| {
    last_sold_duration()
        .map(|d| format_duration(d).to_string())
        .unwrap_or("---".to_string())
});
```

- [ ] **Step 3: Add a `.filter` to `sorted_data`**

After the existing `predicted_time` filter at [analyzer.rs:384-394](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:384), add:

```rust
.filter(move |data| {
    last_sold_duration()
        .map(|max_age| {
            data.inner
                .sale_summary
                .days_since_last_sale
                .and_then(|d| d.to_std().ok())
                .map(|d| d <= max_age)
                .unwrap_or(false)
        })
        .unwrap_or(true)
})
```

- [ ] **Step 4: Add the filter card**

After the "Sale Time Prediction" card at [analyzer.rs:613-630](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:613), add:

```rust
<FilterCard
    title=t_string!(i18n, analyzer_last_sold_within).to_string()
    description=t_string!(i18n, analyzer_last_sold_within_desc).to_string()
>
    <div class="flex flex-col gap-2">
        <div class="text-brand-300">{last_sold_string}</div>
        <input
            class="input"
            placeholder="e.g. 7d"
            title="Accepts formats like 1h 30m, 7d, 1M (month), etc."
            prop:value=move || last_sold_within().unwrap_or_default()
            on:input=move |input| {
                let value = event_target_value(&input);
                set_last_sold_within(Some(value))
            }
        />
    </div>
</FilterCard>
```

- [ ] **Step 5: Add the chip and clear-all wiring**

Add a chip block alongside the existing `max_predicted_time` chip:

```rust
if last_sold_within().is_some() {
    chips.push(view! {
        <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
            {t!(i18n, analyzer_last_sold_lte)} {last_sold_string()}
            <button aria-label="Remove filter" class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_last_sold_within(None)>
                <Icon icon=icondata::MdiClose />
            </button>
        </span>
    }.into_any());
}
```

In the "Clear all" button, add `set_last_sold_within(None);`.

- [ ] **Step 6: Build and verify**

Run:
```
./check_ci.sh
```
Expected: pass.

- [ ] **Step 7: Commit**

```
git add ultros-frontend/ultros-app/src/routes/analyzer.rs ultros-frontend/ultros-app/locales/en.json
git commit -m "feat(analyzer): last-sold-within filter (?last-sold=)"
```

---

## Task 6: Re-tune the preset buttons

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs:1190-1200`
- Modify: `ultros-frontend/ultros-app/locales/en.json`

Replace the three existing presets so each one sets a minimum buy price (the actual 1-gil fix) and a last-sold cap (the actual liveness fix). Add a new leading "Realistic flips" preset.

- [ ] **Step 1: Add the locale strings**

In the analyzer section of `en.json`, add:

```json
    "analyzer_preset_realistic": "Realistic flips",
    "analyzer_preset_big_ticket": "Big-ticket flips",
    "analyzer_preset_volume": "Volume flips",
```

Optionally update the existing `analyzer_preset_300_return`, `analyzer_preset_500_return`, `analyzer_preset_100k_profit` strings if their copy no longer matches their new filter URLs (see Step 2).

- [ ] **Step 2: Replace the preset buttons**

Replace the `<div class="flex flex-wrap gap-4">` block at [analyzer.rs:1190-1200](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:1190) with:

```rust
<div class="flex flex-wrap gap-4">
    <PresetFilterButton
        href="?min-buy=5000&last-sold=7d&roi=30&sort=profit-per-day"
        label=t_string!(i18n, analyzer_preset_realistic).to_string()
    />
    <PresetFilterButton
        href="?min-buy=100000&last-sold=14d&roi=20&sort=profit"
        label=t_string!(i18n, analyzer_preset_big_ticket).to_string()
    />
    <PresetFilterButton
        href="?min-buy=1000&last-sold=3d&sort=profit-per-day"
        label=t_string!(i18n, analyzer_preset_volume).to_string()
    />
    <PresetFilterButton
        href="?min-buy=1000&last-sold=7d&roi=300&profit=0&sort=profit"
        label=t_string!(i18n, analyzer_preset_300_return).to_string()
    />
    <PresetFilterButton
        href="?min-buy=10000&last-sold=1M&roi=500&profit=200000"
        label=t_string!(i18n, analyzer_preset_500_return).to_string()
    />
    <PresetFilterButton
        href="?min-buy=1000&profit=100000"
        label=t_string!(i18n, analyzer_preset_100k_profit).to_string()
    />
</div>
```

- [ ] **Step 3: Build and verify**

Run:
```
./check_ci.sh
```
Expected: pass.

- [ ] **Step 4: Manual smoke check**

Bring up the app locally (`cargo run -p ultros` or the existing dev workflow) and visit `http://localhost:8080/flip-finder/Goblin`. Click each preset and confirm:

- "Realistic flips" lands on a page with no 1-gil buy rows visible.
- "Big-ticket flips" only shows rows with cheapest price ≥ 100,000.
- The existing "300% return" preset no longer surfaces 1-gil rows in the first page of results.

If the dev environment isn't trivially available, document this manual step in the PR description so a reviewer can verify it post-merge — do not skip it.

- [ ] **Step 5: Commit**

```
git add ultros-frontend/ultros-app/src/routes/analyzer.rs ultros-frontend/ultros-app/locales/en.json
git commit -m "feat(analyzer): re-tune presets with min-buy + last-sold guards"
```

---

## Task 7: Final verification

- [ ] **Step 1: Run the full CI check from a clean state**

Run:
```
./check_ci.sh
```
Expected: fmt and clippy pass.

- [ ] **Step 2: Run all analyzer-related unit tests**

Run:
```
cargo test -p ultros-app --lib routes::analyzer
```
Expected: all five tests pass.

- [ ] **Step 3: (Optional) Run E2E smoke**

Run:
```
./scripts/run_e2e.sh
```
Expected: the existing Puppeteer harness still passes. If it doesn't, investigate — the only thing that should have changed visibly is the Flip Finder page; the harness may have a baseline screenshot to refresh.

- [ ] **Step 4: Open the PR**

Push the branch and open a PR titled `feat(analyzer): data-integrity & guidance (Tier 1)`. The body should reference:

- The design doc at [docs/superpowers/specs/2026-05-11-analyzer-data-integrity-design.md](../specs/2026-05-11-analyzer-data-integrity-design.md).
- The before/after evidence from the production data exploration (1-gil sniper rows dominated 71% of the top 500 by ROI before).
- A note about the manual Goblin smoke test (Task 6 Step 4) if it was deferred to review.
