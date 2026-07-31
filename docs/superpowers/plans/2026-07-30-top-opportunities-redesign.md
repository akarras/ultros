# Top Opportunities redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the home-page Top Opportunities card from advertising gil-transfer laundering, and redesign it so a first-time reader can infer the buy-low-elsewhere / sell-here mechanic.

**Architecture:** Eligibility policy moves into a new pure module (`ultros/src/resale_eligibility.rs`) so it is unit-testable without constructing an `AnalyzerService`. `get_best_resale` calls it during pass 1 — *before* the `DEEP_SCAN_TOP_N` truncation — using three signals that exist on 100% of rows: a vendor-price anchor, a velocity floor derived from the 6-sale buffer, and an ROI ceiling. ClickHouse stays a refinement layer only. The card is then rebuilt around a name-owns-its-own-line layout with a route line naming the source world.

**Tech Stack:** Rust, Leptos 0.8 (`view!` macro, `LocalResource`/`Suspense`), leptos-i18n, Tailwind v4, axum, xiv-gen-db.

**Spec:** `docs/superpowers/specs/2026-07-30-top-opportunities-redesign-design.md`

---

## File Structure

**Create:**
- `ultros/src/resale_eligibility.rs` — pure policy functions: conservative median, velocity, vendor anchor, `EligibilityPolicy`. No I/O, no async, fully unit-tested. Keeps `analyzer_service.rs` (already 2229 lines) from growing further.

**Modify:**
- `ultros/src/lib.rs` or `ultros/src/main.rs` — register the new module.
- `ultros/src/analyzer_service.rs` — `SaleHistoryStats` struct, pass-1 wiring, `ResaleStats` + `ResaleOptions` fields.
- `ultros/src/web/api/best_deals.rs` — query params + DTO fields.
- `ultros/src/discord/ffxiv/analyze.rs` — ROI display clamp.
- `ultros-frontend/ultros-app/src/api.rs` — DTO fields + `BestDealsParams`.
- `ultros-frontend/ultros-app/src/components/top_opportunity.rs` — the card.
- `style/tailwind.css` — `.card-link` hover escape.
- `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` — new keys, deletions, and cadence-stub fixes.

---

## Prerequisite: submodules

`cargo check`/`clippy` compile `xiv-gen-db`, whose build script reads
`xiv-gen/ffxiv-datamining/`. A plain `--depth=1` init leaves the nested cn/ko/tc
repos empty and the build panics on `cn/Item.csv`.

- [ ] **Step 1: Verify submodule data is present**

Run:
```bash
ls xiv-gen/ffxiv-datamining/csv/cn/Item.csv xiv-gen/ffxiv-datamining/csv/tc/Item.csv xiv-gen/ffxiv-datamining/csv/ko/csv/Item.csv
```
Expected: all three paths listed, no "No such file".

If missing, run:
```bash
git submodule update --init --recursive --reference /Users/aaronkarras/code/ffxiv-playground/xiv-gen/ffxiv-datamining xiv-gen/ffxiv-datamining
```

Note `ko` nests its CSVs one level deeper (`csv/ko/csv/`) — that is that repo's own layout, not an error.

---

## Task 1: Pure eligibility module — conservative median

**Files:**
- Create: `ultros/src/resale_eligibility.rs`
- Modify: `ultros/src/main.rs` (add `mod resale_eligibility;`)

- [ ] **Step 1: Create the module with a failing test**

Create `ultros/src/resale_eligibility.rs`:

```rust
//! Pure "is this resale row real?" policy.
//!
//! Split out of `analyzer_service` so the thresholds are unit-testable
//! without standing up an `AnalyzerService`. Every signal here is derived
//! from data present on 100% of rows (the 6-sale buffer, listing prices,
//! and xiv-gen vendor prices) — ClickHouse enrichment covers ~7% of traded
//! items and therefore cannot gate default behavior.

/// Multiple of an item's NPC vendor price above which a claimed sale price
/// is arithmetically impossible rather than merely aggressive. Matches the
/// guard already used by the frontend's `real_price`.
pub(crate) const VENDOR_ANCHOR_MULTIPLE: i64 = 100;

/// Minimum span used as the velocity denominator. Guards the degenerate
/// case of six listings cleared in one action, which would divide by zero.
pub(crate) const MIN_VELOCITY_SPAN_DAYS: f32 = 1.0 / 24.0;

/// Median that picks the **lower** middle on even-length input.
///
/// The upper-middle pick resolves a two-sale laundering pair to the higher
/// of the two, which is the worst possible choice for a valuation. Odd
/// lengths and single elements are unaffected.
pub(crate) fn conservative_median(prices: &mut [i32]) -> i32 {
    let idx = (prices.len() - 1) / 2;
    let (_, &mut value, _) = prices.select_nth_unstable(idx);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conservative_median_picks_lower_middle_when_even() {
        assert_eq!(conservative_median(&mut [10, 252_000_000]), 10);
        assert_eq!(conservative_median(&mut [1, 2, 3, 4]), 2);
        assert_eq!(conservative_median(&mut [4, 3, 2, 1]), 2);
    }

    #[test]
    fn conservative_median_unchanged_for_odd_and_single() {
        assert_eq!(conservative_median(&mut [1, 2, 3]), 2);
        assert_eq!(conservative_median(&mut [42]), 42);
        assert_eq!(conservative_median(&mut [5, 1, 3, 2, 4]), 3);
    }
}
```

Add to `ultros/src/main.rs` alongside the other `mod` declarations:

```rust
mod resale_eligibility;
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p ultros resale_eligibility:: -- --nocapture`
Expected: 2 passed. (These pass immediately — `conservative_median` is written alongside its tests because it is three lines and splitting it across steps adds nothing.)

- [ ] **Step 3: Commit**

```bash
git add ultros/src/resale_eligibility.rs ultros/src/main.rs
git commit -m "feat(analyzer): add resale_eligibility module with conservative median"
```

---

## Task 2: Velocity derivation

**Files:**
- Modify: `ultros/src/resale_eligibility.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` module in `ultros/src/resale_eligibility.rs`:

```rust
    #[test]
    fn velocity_is_count_over_span() {
        // 6 sales across 30 days = 0.2/day
        let v = velocity_per_day(6, 30.0).expect("velocity");
        assert!((v - 0.2).abs() < 1e-5, "got {v}");
    }

    #[test]
    fn velocity_clamps_zero_span_instead_of_dividing_by_zero() {
        // Six listings cleared in one action: span 0.
        let v = velocity_per_day(6, 0.0).expect("velocity");
        assert!(v.is_finite(), "velocity must stay finite, got {v}");
        assert!((v - 6.0 / MIN_VELOCITY_SPAN_DAYS).abs() < 1e-3, "got {v}");
    }

    #[test]
    fn velocity_is_none_without_sales() {
        assert_eq!(velocity_per_day(0, 30.0), None);
    }

    #[test]
    fn velocity_of_stale_buffer_is_near_zero() {
        // 2 laundering sales two years apart.
        let v = velocity_per_day(2, 730.0).expect("velocity");
        assert!(v < 0.01, "got {v}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros resale_eligibility::`
Expected: FAIL — `cannot find function velocity_per_day in this scope`

- [ ] **Step 3: Write the implementation**

Add above the `tests` module in `ultros/src/resale_eligibility.rs`:

```rust
/// Recent sales per day from the bounded 6-sale buffer.
///
/// Mirrors `analysis::velocity_per_day` on the frontend so the card and the
/// Flip Finder can never disagree about the same item. Because the buffer
/// holds the *most recent* sales, this estimates the current rate rather
/// than a lifetime average; resolution degrades only at the high end, which
/// does not matter for a floor-style filter.
pub(crate) fn velocity_per_day(count: usize, span_days: f32) -> Option<f32> {
    if count == 0 {
        return None;
    }
    Some(count as f32 / span_days.max(MIN_VELOCITY_SPAN_DAYS))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros resale_eligibility::`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add ultros/src/resale_eligibility.rs
git commit -m "feat(analyzer): derive sale velocity from the recent-sales buffer"
```

---

## Task 3: Vendor anchor and the eligibility policy

**Files:**
- Modify: `ultros/src/resale_eligibility.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` module:

```rust
    fn candidate() -> Candidate {
        Candidate {
            est_sale_price: 21_450,
            return_on_investment: 68.0,
            velocity_per_day: Some(0.4),
            buffer_sale_count: 6,
            vendor_price: 0,
        }
    }

    #[test]
    fn vendor_anchor_rejects_impossible_valuation() {
        // Hempen Coif: ~50 gil vendor price, 42M claimed sale.
        let row = Candidate { est_sale_price: 42_000_000, vendor_price: 50, ..candidate() };
        assert!(!EligibilityPolicy::default().accepts(&row));
    }

    #[test]
    fn vendor_anchor_allows_up_to_the_multiple() {
        let at_limit = Candidate { est_sale_price: 5_000, vendor_price: 50, ..candidate() };
        let over = Candidate { est_sale_price: 5_001, vendor_price: 50, ..candidate() };
        assert!(EligibilityPolicy::default().accepts(&at_limit));
        assert!(!EligibilityPolicy::default().accepts(&over));
    }

    #[test]
    fn vendor_anchor_ignores_non_vendor_items() {
        // price_mid == 0 means "not sold by an NPC vendor".
        let row = Candidate { est_sale_price: 42_000_000, vendor_price: 0, ..candidate() };
        assert!(EligibilityPolicy::default().accepts(&row));
    }

    #[test]
    fn velocity_floor_rejects_below_threshold() {
        let policy = EligibilityPolicy { min_velocity_per_day: Some(0.2), ..Default::default() };
        assert!(!policy.accepts(&Candidate { velocity_per_day: Some(0.19), ..candidate() }));
        assert!(policy.accepts(&Candidate { velocity_per_day: Some(0.2), ..candidate() }));
    }

    #[test]
    fn velocity_floor_rejects_unknown_velocity() {
        let policy = EligibilityPolicy { min_velocity_per_day: Some(0.2), ..Default::default() };
        assert!(!policy.accepts(&Candidate { velocity_per_day: None, ..candidate() }));
    }

    #[test]
    fn buffer_sale_count_floor_applies() {
        let policy = EligibilityPolicy { min_buffer_sales: Some(2), ..Default::default() };
        assert!(!policy.accepts(&Candidate { buffer_sale_count: 1, ..candidate() }));
        assert!(policy.accepts(&Candidate { buffer_sale_count: 2, ..candidate() }));
    }

    #[test]
    fn roi_ceiling_rejects_above_threshold() {
        let policy = EligibilityPolicy { max_roi: Some(5000.0), ..Default::default() };
        assert!(!policy.accepts(&Candidate { return_on_investment: 6_984_380.0, ..candidate() }));
        assert!(policy.accepts(&Candidate { return_on_investment: 5000.0, ..candidate() }));
        // A legitimate cheap-item flip: 715 -> 10,715 gil is a 1400% return.
        assert!(policy.accepts(&Candidate { return_on_investment: 1400.0, ..candidate() }));
    }

    #[test]
    fn default_policy_only_applies_the_vendor_anchor() {
        // The Discord command passes no gates; it must keep seeing everything
        // except arithmetically impossible rows.
        let policy = EligibilityPolicy::default();
        assert!(policy.accepts(&Candidate {
            velocity_per_day: None,
            buffer_sale_count: 1,
            return_on_investment: 900_000.0,
            ..candidate()
        }));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros resale_eligibility::`
Expected: FAIL — `cannot find struct Candidate` / `EligibilityPolicy`

- [ ] **Step 3: Write the implementation**

Add above the `tests` module:

```rust
/// A pass-1 resale row, with everything the policy needs to judge it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Candidate {
    pub(crate) est_sale_price: i32,
    pub(crate) return_on_investment: f32,
    pub(crate) velocity_per_day: Option<f32>,
    pub(crate) buffer_sale_count: u8,
    /// xiv-gen `price_mid`; 0 when the item is not vendor-sold.
    pub(crate) vendor_price: u32,
}

/// Caller-tunable strictness. `Default` applies only the vendor anchor, so
/// existing callers (the Discord `/analyze` command) are unaffected — that
/// anchor rejects arithmetically impossible valuations, not merely
/// aggressive ones.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EligibilityPolicy {
    pub(crate) min_velocity_per_day: Option<f32>,
    pub(crate) min_buffer_sales: Option<u8>,
    pub(crate) max_roi: Option<f32>,
}

impl EligibilityPolicy {
    pub(crate) fn accepts(&self, row: &Candidate) -> bool {
        if row.vendor_price > 0
            && row.est_sale_price as i64 > row.vendor_price as i64 * VENDOR_ANCHOR_MULTIPLE
        {
            return false;
        }
        if let Some(min) = self.min_velocity_per_day
            && row.velocity_per_day.map(|v| v < min).unwrap_or(true)
        {
            return false;
        }
        if let Some(min) = self.min_buffer_sales
            && row.buffer_sale_count < min
        {
            return false;
        }
        if let Some(max) = self.max_roi
            && row.return_on_investment > max
        {
            return false;
        }
        true
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros resale_eligibility::`
Expected: 14 passed.

- [ ] **Step 5: Run clippy on the new module**

Run: `cargo clippy -p ultros --all-targets -- -D warnings`
Expected: no warnings. If clippy rejects the let-chains (`if let Some(x) = y && cond`), rewrite each as a nested `if let`.

- [ ] **Step 6: Commit**

```bash
git add ultros/src/resale_eligibility.rs
git commit -m "feat(analyzer): vendor anchor, velocity floor, and ROI ceiling policy"
```

---

## Task 4: Carry buffer stats through pass 1

**Files:**
- Modify: `ultros/src/analyzer_service.rs:1076-1114` (the `sale_history` map build)

- [ ] **Step 1: Replace the tuple value with a struct**

In `ultros/src/analyzer_service.rs`, add near `ResaleStats` (around line 1564):

```rust
/// Per-item statistics derived from the bounded recent-sales buffer.
/// Everything here has 100% coverage — no ClickHouse dependency.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SaleHistoryStats {
    /// Lower-middle median of the in-window prices.
    pub(crate) median: i32,
    pub(crate) sold_within: SoldWithin,
    /// Lowest and highest in-window price, for the card's "recent" range.
    pub(crate) price_low: i32,
    pub(crate) price_high: i32,
    /// Number of sales in the buffer (not the window) — the velocity basis.
    pub(crate) buffer_sale_count: u8,
    pub(crate) velocity_per_day: Option<f32>,
}
```

Replace the `sale_history` build (currently lines 1076-1114) with:

```rust
        let now = Utc::now().naive_utc();
        let sale_history: BTreeMap<_, _> = sale
            .read()
            .await
            .item_map
            .iter()
            .map(|(i, values)| (i, values, values.iter().collect::<SoldWithin>()))
            .flat_map(|(item, values, sold_within)| {
                let mut prices: smallvec::SmallVec<[i32; SALE_HISTORY_SIZE]> = values
                    .iter()
                    .filter(|sale| {
                        resale_options
                            .filter_sale
                            .as_ref()
                            .map(|sale_within| {
                                let sale_within = Duration::from(sale_within);
                                now.signed_duration_since(sale.sale_date).lt(&sale_within)
                            })
                            .unwrap_or(true)
                    })
                    .map(|sale| sale.price_per_item)
                    .collect();
                if prices.is_empty() {
                    return None;
                }
                let price_low = *prices.iter().min()?;
                let price_high = *prices.iter().max()?;
                let median = crate::resale_eligibility::conservative_median(&mut prices);

                // Velocity uses the whole buffer, not the filtered window:
                // it is a rate estimate, and `sold_within` already carries
                // the windowed view.
                let oldest = values.iter().map(|s| s.sale_date).min();
                let span_days = oldest
                    .map(|o| now.signed_duration_since(o).num_seconds() as f32 / 86_400.0)
                    .unwrap_or(0.0);
                let velocity_per_day =
                    crate::resale_eligibility::velocity_per_day(values.len(), span_days);

                Some((
                    *item,
                    SaleHistoryStats {
                        median,
                        sold_within,
                        price_low,
                        price_high,
                        buffer_sale_count: values.len().min(u8::MAX as usize) as u8,
                        velocity_per_day,
                    },
                ))
            })
            .collect();
```

- [ ] **Step 2: Update the consumer to destructure the struct**

In the `possible_sales` build (currently around line 1130), replace:

```rust
                let (cheapest_history, sold_within) = *sale_history.get(item_key)?;
```

with:

```rust
                let stats = *sale_history.get(item_key)?;
                let cheapest_history = stats.median;
                let sold_within = stats.sold_within;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p ultros`
Expected: compiles clean. (`ResaleStats` does not carry the new fields yet — Task 5.)

- [ ] **Step 4: Commit**

```bash
git add ultros/src/analyzer_service.rs
git commit -m "refactor(analyzer): carry buffer stats through pass 1 as a struct"
```

---

## Task 5: Apply the policy before truncation

**Files:**
- Modify: `ultros/src/analyzer_service.rs` — `ResaleStats`, `ResaleOptions`, `possible_sales` filter chain

- [ ] **Step 1: Add the new fields to `ResaleStats` and `ResaleOptions`**

In `ultros/src/analyzer_service.rs`, add to `ResaleStats` (after `world_id`, around line 1571):

```rust
    /// Buffer-derived, 100% coverage. `None` when the buffer is empty.
    pub(crate) velocity_per_day: Option<f32>,
    pub(crate) buffer_sale_count: u8,
    pub(crate) recent_price_low: i32,
    pub(crate) recent_price_high: i32,
```

Add to `ResaleOptions` (after `include_suspicious`, around line 1592):

```rust
    /// Reject rows selling slower than this. `None` disables the floor.
    pub(crate) min_velocity_per_day: Option<f32>,
    /// Reject rows with fewer than this many sales in the buffer.
    pub(crate) min_buffer_sales: Option<u8>,
    /// Reject rows above this ROI percentage. Covers the velocity floor's
    /// blind spot: laundering compressed into a short burst.
    pub(crate) max_roi: Option<f32>,
```

- [ ] **Step 2: Populate the fields and apply the policy**

In the `possible_sales` `flat_map` (around line 1136), extend the `ResaleStats` construction:

```rust
                Some(ResaleStats {
                    profit,
                    item_id: item_key.item_id,
                    hq: item_key.hq,
                    return_on_investment,
                    world_id: cheapest_price.world_id,
                    sold_within,
                    velocity_per_day: stats.velocity_per_day,
                    buffer_sale_count: stats.buffer_sale_count,
                    recent_price_low: stats.price_low,
                    recent_price_high: stats.price_high,
                    // Pass-1 defaults; the deep-scan pass fills these in.
                    confidence_band: ultros_api_types::trends::ConfidenceBand::Unknown,
                    vwap_30d: 0,
                    sample_size_30d: 0,
                    launder_suspicion: 0.0,
                })
```

The policy must be applied **inside** the same `flat_map`, not as a later
`.filter()` — `est_sale_price` is a local there and is not stored on `ResaleStats`,
so a downstream filter would have to reconstruct it and get it wrong. Build the
policy once above the iterator chain:

```rust
        let policy = crate::resale_eligibility::EligibilityPolicy {
            min_velocity_per_day: resale_options.min_velocity_per_day,
            min_buffer_sales: resale_options.min_buffer_sales,
            max_roi: resale_options.max_roi,
        };
        let game_data = xiv_gen_db::data();
```

Then inside the `flat_map`, immediately after `est_sale_price` and
`return_on_investment` are known and **before** the `Some(ResaleStats { .. })`:

```rust
                let return_on_investment =
                    ((est_sale_price as f32) / (cheapest_price.price as f32) * 100.0) - 100.0;
                let vendor_price = game_data
                    .items
                    .get(&xiv_gen::ItemId(item_key.item_id))
                    .map(|i| i.price_mid)
                    .unwrap_or(0);
                if !policy.accepts(&crate::resale_eligibility::Candidate {
                    est_sale_price,
                    return_on_investment,
                    velocity_per_day: stats.velocity_per_day,
                    buffer_sale_count: stats.buffer_sale_count,
                    vendor_price,
                }) {
                    return None;
                }
```

and use the `return_on_investment` local in the struct literal rather than
recomputing it inline.

Because this is inside the `flat_map` that produces `possible_sales`, it runs
**before** the `DEEP_SCAN_TOP_N` truncation at line 1192 — which is the point: the
deep-scan budget is then spent on rows that already qualify.

- [ ] **Step 3: Verify it compiles and existing tests still pass**

Run: `cargo test -p ultros`
Expected: all existing tests pass. `ResaleOptions` derives `Default`, so the Discord call site at `ultros/src/discord/ffxiv/analyze.rs:66` continues to compile with the new `None` fields.

- [ ] **Step 4: Commit**

```bash
git add ultros/src/analyzer_service.rs
git commit -m "feat(analyzer): gate resale rows on vendor anchor, velocity, and ROI"
```

---

## Task 6: Expose the new fields and gates over HTTP

**Files:**
- Modify: `ultros/src/web/api/best_deals.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module at the bottom of `ultros/src/web/api/best_deals.rs`:

```rust
    #[test]
    fn eligibility_params_extract_alongside_existing_ones() {
        let q = extract(
            "min_profit=10000&filter_sale=Week&limit=20&show_suspicious=0\
             &min_velocity=0.2&min_buffer_sales=2&max_roi=5000",
        )
        .expect("eligibility params must extract");
        assert_eq!(q.min_profit, Some(10000));
        assert_eq!(q.min_velocity, Some(0.2));
        assert_eq!(q.min_buffer_sales, Some(2));
        assert_eq!(q.max_roi, Some(5000.0));
    }

    #[test]
    fn eligibility_params_are_optional() {
        let q = extract("limit=10").expect("omitted params must extract");
        assert_eq!(q.min_velocity, None);
        assert_eq!(q.min_buffer_sales, None);
        assert_eq!(q.max_roi, None);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros best_deals::`
Expected: FAIL — `no field min_velocity on type BestDealsQuery`

- [ ] **Step 3: Add the query fields, DTO fields, and wiring**

In `ultros/src/web/api/best_deals.rs`, add to `BestDealsQuery`:

```rust
    /// Reject rows selling slower than this many per day.
    pub(crate) min_velocity: Option<f32>,
    /// Reject rows with fewer than this many sales in the recent buffer.
    pub(crate) min_buffer_sales: Option<u8>,
    /// Reject rows above this ROI percentage.
    pub(crate) max_roi: Option<f32>,
```

Add to `ResaleStatsDto`:

```rust
    pub(crate) velocity_per_day: Option<f32>,
    pub(crate) buffer_sale_count: u8,
    pub(crate) recent_price_low: i32,
    pub(crate) recent_price_high: i32,
```

Extend `impl From<ResaleStats> for ResaleStatsDto` with:

```rust
            velocity_per_day: stats.velocity_per_day,
            buffer_sale_count: stats.buffer_sale_count,
            recent_price_low: stats.recent_price_low,
            recent_price_high: stats.recent_price_high,
```

Extend the `ResaleOptions` construction in `get_best_deals`:

```rust
        min_velocity_per_day: query.min_velocity,
        min_buffer_sales: query.min_buffer_sales,
        max_roi: query.max_roi,
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros best_deals::`
Expected: 5 passed (3 existing + 2 new).

- [ ] **Step 5: Commit**

```bash
git add ultros/src/web/api/best_deals.rs
git commit -m "feat(api): expose eligibility gates and buffer stats on best_deals"
```

---

## Task 7: Clamp ROI in the Discord command

**Files:**
- Modify: `ultros/src/discord/ffxiv/analyze.rs:77-85`

- [ ] **Step 1: Clamp the displayed value**

The command formats `sale.return_on_investment` directly, so a laundered row
prints an unreadable figure. Replace the value passed into the row format with a
clamped one. In `ultros/src/discord/ffxiv/analyze.rs`, immediately before the
`format!` that emits `sale.return_on_investment` (line 84), introduce:

```rust
                        // Beyond this the exact figure carries no decision
                        // value. Mirrors ROI_DISPLAY_CEILING on the frontend.
                        let roi = sale.return_on_investment.min(100_000.0);
```

and use `roi` in place of `sale.return_on_investment` in that `format!`.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p ultros`
Expected: compiles clean.

- [ ] **Step 3: Commit**

```bash
git add ultros/src/discord/ffxiv/analyze.rs
git commit -m "fix(discord): clamp ROI display in the analyze command"
```

---

## Task 8: Frontend API surface

**Files:**
- Modify: `ultros-frontend/ultros-app/src/api.rs:119-175`

- [ ] **Step 1: Add the DTO fields**

In `ultros-frontend/ultros-app/src/api.rs`, add to `ResaleStatsDto`:

```rust
    #[serde(default)]
    pub(crate) velocity_per_day: Option<f32>,
    #[serde(default)]
    pub(crate) buffer_sale_count: u8,
    #[serde(default)]
    pub(crate) recent_price_low: i32,
    #[serde(default)]
    pub(crate) recent_price_high: i32,
```

- [ ] **Step 2: Add the request params**

Add to `BestDealsParams`:

```rust
    pub min_velocity: Option<f32>,
    pub min_buffer_sales: Option<u8>,
    pub max_roi: Option<f32>,
```

And in `get_best_deals`, after the `show_suspicious` block:

```rust
    if let Some(v) = params.min_velocity {
        qs.push(format!("min_velocity={v}"));
    }
    if let Some(n) = params.min_buffer_sales {
        qs.push(format!("min_buffer_sales={n}"));
    }
    if let Some(r) = params.max_roi {
        qs.push(format!("max_roi={r}"));
    }
```

Change `Vec::with_capacity(4)` to `Vec::with_capacity(7)`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p ultros-app`
Expected: compiles clean.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/api.rs
git commit -m "feat(app): thread eligibility params and buffer stats through the API client"
```

---

## Task 9: i18n keys

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json`

Per CLAUDE.md every key must exist in all seven locales with a real translation.
The card renders these on the landing page, so English stubs are user-visible.

- [ ] **Step 1: Add the new keys**

Add to each locale file:

| key | en | fr | de | ja |
| --- | --- | --- | --- | --- |
| `top_opportunities_route` | `Buy on {{source}} → list on {{home}}` | `Acheter sur {{source}} → vendre sur {{home}}` | `Auf {{source}} kaufen → auf {{home}} anbieten` | `{{source}}で購入 → {{home}}で出品` |
| `top_opportunities_profit_each` | `Profit each` | `Bénéfice unitaire` | `Gewinn pro Stück` | `1個あたりの利益` |
| `top_opportunities_recent_range` | `recent {{low}}–{{high}}` | `récent {{low}}–{{high}}` | `zuletzt {{low}}–{{high}}` | `直近 {{low}}–{{high}}` |
| `top_opportunities_vwap_30d` | `30d avg {{price}}` | `moy. 30 j {{price}}` | `30-Tage-Ø {{price}}` | `30日平均 {{price}}` |
| `top_opportunities_empty_title` | `Nothing worth flipping on {{world}} right now` | `Rien à revendre sur {{world}} pour le moment` | `Auf {{world}} lohnt sich derzeit nichts` | `現在{{world}}に転売の妙味はありません` |
| `top_opportunities_empty_body` | `Only items that actually sell show up here, so a quiet market means an empty card.` | `Seuls les objets qui se vendent vraiment apparaissent ici : un marché calme donne une carte vide.` | `Hier erscheinen nur Gegenstände, die sich wirklich verkaufen — ein ruhiger Markt bleibt leer.` | `実際に売れている商品のみを表示するため、市場が静かなときは空欄になります。` |
| `top_opportunities_empty_cta` | `Browse everything in Flip Finder` | `Tout parcourir dans Flip Finder` | `Alles im Flip Finder ansehen` | `Flip Finderですべて見る` |
| `top_opportunities_error` | `Couldn't load opportunities.` | `Impossible de charger les opportunités.` | `Gelegenheiten konnten nicht geladen werden.` | `チャンスを読み込めませんでした。` |

| key | cn | ko | tc |
| --- | --- | --- | --- |
| `top_opportunities_route` | `在{{source}}购买 → 在{{home}}出售` | `{{source}}에서 구매 → {{home}}에서 판매` | `在{{source}}購買 → 在{{home}}出售` |
| `top_opportunities_profit_each` | `单件利润` | `개당 수익` | `單件利潤` |
| `top_opportunities_recent_range` | `近期 {{low}}–{{high}}` | `최근 {{low}}–{{high}}` | `近期 {{low}}–{{high}}` |
| `top_opportunities_vwap_30d` | `30天均价 {{price}}` | `30일 평균 {{price}}` | `30天均價 {{price}}` |
| `top_opportunities_empty_title` | `目前{{world}}没有值得倒卖的物品` | `지금 {{world}}에는 되팔 만한 물건이 없습니다` | `目前{{world}}沒有值得倒賣的物品` |
| `top_opportunities_empty_body` | `这里只显示真正有成交的物品，市场冷清时自然为空。` | `실제로 팔리는 물건만 표시하므로 시장이 한산하면 비어 있습니다.` | `這裡只顯示真正有成交的物品，市場冷清時自然為空。` |
| `top_opportunities_empty_cta` | `在 Flip Finder 中查看全部` | `Flip Finder에서 전체 보기` | `在 Flip Finder 中查看全部` |
| `top_opportunities_error` | `无法加载机会。` | `기회를 불러오지 못했습니다.` | `無法載入機會。` |

- [ ] **Step 2: Delete the superseded keys**

Remove `top_opportunities_roi` and `top_opportunities_empty` from all seven files.

- [ ] **Step 3: Fix the cadence stubs the card now surfaces**

`sales_cadence_fast`, `sales_cadence_steady`, `sales_cadence_slow`,
`sales_cadence_not_enough_data`, and `sales_cadence_label_with_velocity` are
currently English text in all six non-English locales. The card promotes
`SalesCadenceBadge` onto the landing page, so fix them here:

| key | fr | de | ja | cn | ko | tc |
| --- | --- | --- | --- | --- | --- | --- |
| `sales_cadence_fast` | `Vente rapide` | `Schneller Umschlag` | `早い回転` | `快速成交` | `빠른 회전` | `快速成交` |
| `sales_cadence_steady` | `Vente régulière` | `Stetiger Umschlag` | `安定した回転` | `稳定成交` | `꾸준한 회전` | `穩定成交` |
| `sales_cadence_slow` | `Vente lente` | `Langsamer Umschlag` | `遅い回転` | `成交缓慢` | `느린 회전` | `成交緩慢` |
| `sales_cadence_not_enough_data` | `Données insuffisantes` | `Zu wenig Daten` | `データ不足` | `数据不足` | `데이터 부족` | `資料不足` |
| `sales_cadence_label_with_velocity` | `{{label}} ({{velocity}}/jour)` | `{{label}} ({{velocity}}/Tag)` | `{{label}}（{{velocity}}/日）` | `{{label}}（{{velocity}}/天）` | `{{label}} ({{velocity}}/일)` | `{{label}}（{{velocity}}/天）` |

- [ ] **Step 4: Verify every locale has the same key set**

Run:
```bash
python3 -c "
import json
sets={l:set(json.load(open(f'ultros-frontend/ultros-app/locales/{l}.json'))) for l in ['en','fr','de','ja','cn','ko','tc']}
base=sets['en']
bad=False
for l,s in sets.items():
    if s!=base:
        bad=True
        print(l,'missing:',sorted(base-s),'extra:',sorted(s-base))
print('MISMATCH' if bad else 'all locales aligned', len(base),'keys')
"
```
Expected: `all locales aligned` and `top_opportunities_roi` absent.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/locales/
git commit -m "i18n(top-opportunities): add route/empty/error keys, translate cadence labels"
```

---

## Task 10: Hover escape for card-sized links

**Files:**
- Modify: `style/tailwind.css:813-822`

- [ ] **Step 1: Add `.card-link` to the exclusion list**

The global rule underlines every text node inside an `<a>` on hover, which on a
card-sized link means eleven simultaneous underlines. Replace the two selectors at
`style/tailwind.css:813` and `style/tailwind.css:820` so both also exclude
`.card-link`:

```css
a:not(.nav-link):not(.btn):not(.btn-primary):not(.btn-secondary):not(
        .btn-ghost
    ):not(.card-link) {
    @apply hover:underline rounded-md;
    color: var(--brand-fg);
    background-color: transparent;
}
a:not(.nav-link):not(.btn):not(.btn-primary):not(.btn-secondary):not(
        .btn-ghost
    ):not(.card-link):hover {
    color: color-mix(in srgb, var(--brand-ring) 60%, var(--brand-fg));
    background-color: color-mix(in srgb, var(--brand-ring) 22%, transparent);
}
```

Apply the same `:not(.card-link)` to the light-mode tuning block that follows at
`style/tailwind.css:828`.

- [ ] **Step 2: Commit**

```bash
git add style/tailwind.css
git commit -m "style: let card-sized links opt out of the global hover underline"
```

---

## Task 11: Rebuild the card

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/top_opportunity.rs` (full rewrite)

- [ ] **Step 1: Replace the file**

```rust
//! Home-page Top Opportunities card.
//!
//! One featured flip plus four compact follow-ups, ranked by absolute
//! profit among rows that cleared the server-side eligibility gates
//! (vendor anchor, velocity floor, ROI ceiling — see
//! `ultros/src/resale_eligibility.rs`). The card asks for those gates
//! explicitly rather than relying on defaults, and its "view all" link
//! carries the same ranking and floor into the Flip Finder so the two
//! surfaces agree about the same item.
//!
//! Buy / Sell aren't on the wire; we derive them from `profit + ROI`:
//!     buy  = profit * 100 / ROI
//!     sell = buy + profit

use leptos::prelude::*;
use leptos_router::components::A;
use ultros_api_types::world_helper::AnySelector;

use crate::{
    analysis::get_sales_cadence,
    api::{BestDealsParams, ResaleStatsDto, get_best_deals},
    components::{gil::Gil, item_icon::ItemIcon, sales_cadence_badge::SalesCadenceBadge},
    global_state::{LocalWorldData, xiv_data::tracked_data},
    i18n::*,
};
use ultros_api_types::icon_size::IconSize;

/// How many deals to render in the card (1 featured + N-1 compact).
const VISIBLE_DEALS: usize = 5;
/// Matches the Flip Finder default so the handoff link applies the same floor.
const MIN_VELOCITY: f32 = 0.2;
const MIN_BUFFER_SALES: u8 = 2;
const MAX_ROI: f32 = 5000.0;

/// Resolve a world id to its name. Mirrors `WorldName`'s lookup, but returns
/// a `String` because the route line interpolates it into a translated
/// sentence rather than rendering it as its own element.
///
/// Named `lookup_world_name` rather than `world_name` because both deal
/// components take a `world_name: String` prop that would shadow it.
fn lookup_world_name(world_id: i32) -> Option<String> {
    use_context::<LocalWorldData>()?
        .0
        .ok()?
        .lookup_selector(AnySelector::World(world_id))
        .map(|w| w.get_name().to_string())
}

fn item_name(item_id: i32, i18n: I18nContext<Locale, I18nKeys>) -> String {
    tracked_data()
        .items
        .get(&xiv_gen::ItemId(item_id))
        .map(|i| i.name.as_str().to_string())
        .unwrap_or_else(|| t_string!(i18n, unknown_item).to_string())
}

fn derive_buy_sell(deal: &ResaleStatsDto) -> (i32, i32) {
    let buy = if deal.return_on_investment > 0.0 {
        (deal.profit as f64 * 100.0 / deal.return_on_investment as f64).round() as i32
    } else {
        0
    };
    let sell = buy + deal.profit;
    (buy, sell)
}

#[component]
pub fn TopOpportunities(world: Signal<Option<String>>) -> impl IntoView {
    let i18n = use_i18n();
    let deals = LocalResource::new(move || {
        let w = world.get();
        async move {
            let w = w?;
            let params = BestDealsParams {
                min_profit: Some(10_000),
                filter_sale: Some("Week"),
                limit: Some(20),
                show_suspicious: Some(false),
                min_velocity: Some(MIN_VELOCITY),
                min_buffer_sales: Some(MIN_BUFFER_SALES),
                max_roi: Some(MAX_ROI),
            };
            // Err and empty are different states — an outage must not render
            // as "the market is quiet".
            Some(get_best_deals(&w, params).await.map(|mut v| {
                // FE-side launder defense-in-depth: a flipped server flag
                // can't expose junk on the home page.
                v.retain(|d| {
                    d.return_on_investment > 0.0 && d.profit > 0 && d.launder_suspicion <= 0.7
                });
                v.into_iter().take(VISIBLE_DEALS).collect::<Vec<_>>()
            }))
        }
    });

    let flip_finder_href = move || {
        world
            .get()
            .map(|w| format!("/flip-finder/{w}?sort=profit&vel={MIN_VELOCITY}"))
            .unwrap_or_else(|| "/flip-finder".to_string())
    };

    view! {
        <section class="dashboard-section">
            <header class="flex items-baseline justify-between mb-3">
                <h2 class="dashboard-section-title">{t!(i18n, top_opportunities_title)}</h2>
                <A
                    href=flip_finder_href
                    attr:class="text-xs text-[color:var(--accent)] hover:underline"
                >
                    {t!(i18n, top_opportunities_view_all)}
                </A>
            </header>
            <Suspense fallback=move || view! {
                <div class="space-y-2">
                    <div class="h-32 rounded bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] animate-pulse" />
                    {(0..4).map(|_| view! {
                        <div class="h-12 rounded bg-[color:color-mix(in_srgb,var(--color-text)_3%,transparent)] animate-pulse" />
                    }).collect_view()}
                </div>
            }>
                {move || {
                    let world_str = world.get().unwrap_or_default();
                    deals.get().map(|maybe| match maybe {
                        Some(Ok(list)) if !list.is_empty() => {
                            let mut iter = list.into_iter();
                            let featured = iter.next();
                            let rest: Vec<_> = iter.collect();
                            view! {
                                <div class="flex flex-col gap-1">
                                    {featured.map(|d| view! {
                                        <FeaturedDeal deal=d world_name=world_str.clone() />
                                    })}
                                    {rest
                                        .into_iter()
                                        .map(|d| view! {
                                            <CompactDeal deal=d world_name=world_str.clone() />
                                        })
                                        .collect_view()}
                                </div>
                            }.into_any()
                        },
                        Some(Err(_)) => view! {
                            <div class="text-sm text-[color:var(--color-text-muted)] py-4">
                                {t!(i18n, top_opportunities_error)}
                            </div>
                        }.into_any(),
                        _ => view! { <EmptyState world=world /> }.into_any(),
                    })
                }}
            </Suspense>
        </section>
    }
}

#[component]
fn EmptyState(world: Signal<Option<String>>) -> impl IntoView {
    let i18n = use_i18n();
    let world_label = move || world.get().unwrap_or_default();
    let browse_href = move || {
        world
            .get()
            .map(|w| format!("/flip-finder/{w}?sort=profit&vel=0"))
            .unwrap_or_else(|| "/flip-finder".to_string())
    };
    view! {
        <div class="py-6 flex flex-col gap-2">
            <div class="text-sm font-medium text-[color:var(--color-text)]">
                {move || t_string!(i18n, top_opportunities_empty_title, world = world_label())}
            </div>
            <div class="text-xs text-[color:var(--color-text-muted)] max-w-prose">
                {t!(i18n, top_opportunities_empty_body)}
            </div>
            <A href=browse_href attr:class="text-xs text-[color:var(--accent)] hover:underline">
                {t!(i18n, top_opportunities_empty_cta)}
            </A>
        </div>
    }
}

/// Route line + the credibility anchor. Shared shape between both rows so
/// the two never drift.
#[component]
fn RouteLine(source_world_id: i32, home_world: String) -> impl IntoView {
    let i18n = use_i18n();
    let source = lookup_world_name(source_world_id);
    match source {
        Some(source) => view! {
            <div class="text-xs text-[color:var(--color-text-muted)] mt-1">
                {t_string!(i18n, top_opportunities_route, source = source, home = home_world)}
            </div>
        }
        .into_any(),
        None => ().into_any(),
    }
}

#[component]
fn FeaturedDeal(deal: ResaleStatsDto, world_name: String) -> impl IntoView {
    let i18n = use_i18n();
    let item_id = deal.item_id;
    let name = item_name(item_id, i18n);
    let (buy, sell) = derive_buy_sell(&deal);
    let href = format!("/item/{world_name}/{item_id}");
    let buy_sell_label = format!(
        "{} {} — {} {}",
        t_string!(i18n, top_opportunities_buy),
        buy,
        t_string!(i18n, top_opportunities_sell),
        sell
    );

    // 100%-coverage anchor by default; ClickHouse upgrades it where present.
    let anchor = if deal.vwap_30d > 0 {
        t_string!(i18n, top_opportunities_vwap_30d, price = deal.vwap_30d.to_string()).to_string()
    } else {
        t_string!(
            i18n,
            top_opportunities_recent_range,
            low = deal.recent_price_low.to_string(),
            high = deal.recent_price_high.to_string()
        )
        .to_string()
    };

    view! {
        <a
            href=href
            class="card-link block rounded p-3 bg-[color:color-mix(in_srgb,var(--brand-ring)_6%,transparent)] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] transition-colors group"
        >
            <div class="flex gap-3 items-start">
                <div class="shrink-0">
                    <ItemIcon item_id icon_size=IconSize::Large />
                </div>
                <div class="min-w-0 flex-1">
                    <div class="text-base font-semibold text-[color:var(--color-text)] leading-snug line-clamp-2 group-hover:underline">
                        {name}
                    </div>
                    <RouteLine source_world_id=deal.world_id home_world=world_name.clone() />
                </div>
            </div>
            <div class="flex items-end justify-between gap-3 mt-3 pt-3 border-t border-[color:var(--line)]">
                <div class="min-w-0">
                    <div class="text-[10px] uppercase tracking-wider text-[color:var(--color-text-muted)]">
                        {t!(i18n, top_opportunities_profit_each)}
                    </div>
                    <div class="text-2xl font-semibold font-mono text-emerald-300 leading-none tabular-nums">
                        <Gil amount=deal.profit />
                    </div>
                    <div
                        class="text-[11px] text-[color:var(--color-text-muted)] font-mono mt-1"
                        aria-label=buy_sell_label
                    >
                        <Gil amount=buy />" → "<Gil amount=sell />
                    </div>
                </div>
                <div class="flex flex-col items-end gap-1 shrink-0">
                    {deal.velocity_per_day.map(|v| {
                        let cadence = get_sales_cadence(v, deal.buffer_sale_count as usize);
                        view! { <SalesCadenceBadge cadence sales_per_day=v compact=true /> }
                    })}
                    <span class="text-[11px] text-[color:var(--color-text-muted)] font-mono">
                        {anchor}
                    </span>
                </div>
            </div>
        </a>
    }
}

#[component]
fn CompactDeal(deal: ResaleStatsDto, world_name: String) -> impl IntoView {
    let i18n = use_i18n();
    let item_id = deal.item_id;
    let name = item_name(item_id, i18n);
    let (buy, sell) = derive_buy_sell(&deal);
    let href = format!("/item/{world_name}/{item_id}");
    let source = lookup_world_name(deal.world_id).unwrap_or_default();
    let velocity = deal
        .velocity_per_day
        .map(|v| t_string!(i18n, sales_cadence_compact, velocity = format!("{v:.1}")).to_string())
        .unwrap_or_default();
    let buy_sell_label = format!(
        "{} {} — {} {}",
        t_string!(i18n, top_opportunities_buy),
        buy,
        t_string!(i18n, top_opportunities_sell),
        sell
    );

    view! {
        <a
            href=href
            class="card-link grid grid-cols-[auto_1fr_auto] items-center gap-3 py-2 px-1 rounded border-t border-[color:var(--line)] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)] transition-colors group"
        >
            <div class="shrink-0">
                <ItemIcon item_id icon_size=IconSize::Small />
            </div>
            <div class="min-w-0 flex flex-col gap-0.5">
                <div class="text-sm font-medium text-[color:var(--color-text)] truncate group-hover:underline">
                    {name}
                </div>
                <div class="text-[10px] text-[color:var(--color-text-muted)] truncate">
                    {source}{move || if velocity.is_empty() { String::new() } else { format!(" · {velocity}") }}
                </div>
            </div>
            <div class="flex flex-col items-end text-right shrink-0">
                <span class="text-sm font-semibold font-mono text-emerald-300 tabular-nums">
                    <Gil amount=deal.profit />
                </span>
                <span
                    class="text-[10px] text-[color:var(--color-text-muted)] font-mono"
                    aria-label=buy_sell_label
                >
                    <Gil amount=buy />" → "<Gil amount=sell />
                </span>
            </div>
        </a>
    }
}
```

- [ ] **Step 2: Register `sales_cadence_badge` if it isn't already public**

Run: `grep -n "sales_cadence_badge" ultros-frontend/ultros-app/src/components/mod.rs`
Expected: a `pub mod sales_cadence_badge;` line. If absent, add it.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p ultros-app`
Expected: compiles clean. Two likely fixes:
- `I18nContext<Locale, I18nKeys>` needs `use leptos_i18n::I18nContext;` — copy the import style from `ultros-frontend/ultros-app/src/sales_cadence.rs:3`.
- If `t_string!` with interpolation returns a non-`String` type, wrap with `.to_string()` as the surrounding code already does.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/top_opportunity.rs ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(top-opportunities): name-first layout, route line, cadence, split states"
```

---

## Task 12: Full CI and visual verification

- [ ] **Step 1: Format**

Run: `cargo fmt --all`
Then: `cargo fmt --all -- --check`
Expected: no output.

- [ ] **Step 2: Full CI**

Run: `./check_ci.sh`
Expected: clean. Fix any clippy warning by changing the code, not by adding `#[allow]`.

- [ ] **Step 3: Confirm ROI is gone from the card**

Run: `grep -n "return_on_investment\|roi" ultros-frontend/ultros-app/src/components/top_opportunity.rs`
Expected: only the `derive_buy_sell` arithmetic and the `retain` guard — no rendered ROI.

- [ ] **Step 4: Visual check**

Start the app and load the home page with a home world set. Confirm:
- a long item name (e.g. Archeo Kingdom Partisan) renders in full, not truncated
- the route line names a source world different from the home world
- no ROI percentage appears anywhere on the card
- hovering the featured card tints the background and underlines only the item name
- the profit figure is plausible (four to six digits, not nine)

- [ ] **Step 5: Commit any fixes**

```bash
git add -A
git commit -m "chore: fmt and clippy fixes for top opportunities redesign"
```

---

## Notes for the implementer

- **Do not** reintroduce a ClickHouse-dependent gate. `vwap_30d`, `sample_size_30d`,
  and `confidence_band` are absent on ~93% of rows; anything gating on them
  silently no-ops. They may only refine what is already displayed.
- The policy filter must stay **inside** the iterator chain that builds
  `possible_sales`, before the `DEEP_SCAN_TOP_N` truncation. Moving it after the
  truncation reintroduces the original bug in a subtler form: the deep-scan budget
  goes back to being spent on laundering.
- `ResaleOptions::default()` must keep all three gates at `None` so the Discord
  `/analyze` command is unaffected.
- The spec's `last 6: {{low}}–{{high}}` wording became `recent {{low}}–{{high}}`
  here — the window filter means the count varies, so naming a fixed count would
  sometimes be wrong.

## Known merge conflict — `calculate_profit_and_roi`

A parallel branch (`claude/flip-profit-roi-mismatch-9f666c`, also based on
`c4df0cb7`) introduces `calculate_profit_and_roi(est_sale_price, cheapest_price)
-> (i32, f32)` in `analyzer_service.rs`. It applies a 5% market tax (post-tax sale
= 95% of estimate) and returns `roi = 0.0` when the cost basis is non-positive.

It replaces the same two lines this plan rewrote in `get_best_resale`. Whichever
lands second resolves it like this — keep both changes:

```rust
let (profit, return_on_investment) =
    calculate_profit_and_roi(est_sale_price, cheapest_price.price);
```

then feed that `return_on_investment` into the `Candidate` as this plan already
does. The eligibility gates are unaffected in kind: the ROI ceiling and vendor
anchor still apply, just against post-tax figures.

**Do not skip their tax change to avoid the conflict.** If Flip Finder shows
post-tax profit and this card shows pre-tax, the `?sort=profit&vel=0.2` handoff
link opens a page whose numbers disagree with the card — which is the exact
coherence problem that link was added to solve.
