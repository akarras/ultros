# Item Page Verdicts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a sell verdict (fast/patient list price, undercut wall, days of stock) and a craft-or-buy verdict to the top of `/item/:world/:id`.

**Architecture:** All arithmetic lives in a new pure module `ultros-calc/src/verdict.rs` (no Leptos, unit-tested). A new `routes/item_view_verdicts.rs` holds three Leptos components (`ItemVerdicts`, `SellVerdictCard`, `CraftVerdictCard`) that adapt the page's existing listings payload and the `CheapestPrices` context into those functions, mounted inside `#overview` after `DecisionHeader`. No new requests, no backend changes.

**Tech Stack:** Rust 2024, Leptos 0.8 (SSR + hydrate), leptos-i18n, Tailwind classes, `ultros-crafting::compute_cost`.

**Spec:** `docs/superpowers/specs/2026-09-22-item-verdicts-design.md`

## Global Constraints

- Every user-facing string goes through leptos-i18n; new keys are prefixed `item_verdict_*` and added to all seven locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) in `ultros-frontend/ultros-i18n/locales/` with real translations.
- Thresholds are named constants at the top of `verdict.rs`: `WALL_STEP = 1.05`, `RATE_WINDOW_DAYS = 7`, `MIN_RATE_SALES = 3`, `FLOOR_LOW = 0.7`, `FLOOR_HIGH = 1.5`, `EVEN_BAND = 0.03`.
- Undercutting is per world: the sell verdict never aggregates listings across worlds.
- Craft pricing uses the viewer's price zone (`CheapestPrices`), `max_subcraft_depth: 2`, per-unit cost `cost / amount_result.max(1)`, and honors the `CRAFT_OPTIONS` cookie (exclude crystals, use on-hand).
- Buy price = zone cheapest at the headline quality (HQ falls back to NQ, labelled); for an NQ verdict only, `min(zone, vendor)`.
- Anything depending on `now` or on `CheapestPrices` renders only after a `hydrated` flag flips in an `Effect` (SSR/CSR DOM must match).
- Components read i18n with `crate::i18n_fallback::use_i18n_or_default()` (GlitchTip #7294).
- `./check_ci.sh` must pass before every commit (`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`). Read its real exit code: `./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"` (use the session scratchpad, not `/tmp`).
- Windows: if `openssl-sys` rebuilds, prepend `/c/Strawberry/perl/bin:/c/Strawberry/c/bin:` to `PATH` and set `OPENSSL_RUST_USE_NASM=0`. For ultros-app test binaries set `CARGO_PROFILE_DEV_DEBUG=0` from the first build (4 GiB rlib link limit).

## Review Focus

1. **`?exclude-worlds=` removes the sell world** (e.g. a DC page with the home world excluded): the card must show the "pick a world" hint, not "No listings on <home>". Pinned by `sell_world_ignores_excluded_worlds` in Task 4.
2. **A slow item whose recent sales are all older than 7 days**: the rate must read "too few sales", not zero units/day or infinite days of stock. Pinned by `sale_rate_ignores_sales_older_than_the_window` in Task 1.
3. **Client clock behind the server** (sales stamped a minute in the future): the rate must stay finite and positive. Pinned by `sale_rate_survives_sales_in_the_future` in Task 1.
4. **Everything covered by on-hand stock** (craft cost 0 with "Use on-hand"): the verdict must be "craft saves 100%", not "incomplete". Pinned by `craft_verdict_zero_cost_from_on_hand_saves_everything` in Task 2.
5. **World list failed to load** (`LocalWorldData` error): the sell card must render the hint rather than panic. Pinned by `item_verdicts_builds_without_contexts` in Task 4 (no `LocalWorldData` provided).

---

## File Structure

| File | Responsibility |
|---|---|
| `ultros-frontend/ultros-calc/src/verdict.rs` (create) | Pure types + functions: headline quality, wall/gap, sale rate, sell verdict, buy price, craft verdict. |
| `ultros-frontend/ultros-calc/src/lib.rs` (modify) | `pub mod verdict;` |
| `ultros-frontend/ultros-i18n/locales/{en,fr,de,ja,cn,ko,tc}.json` (modify) | 29 `item_verdict_*` keys each. |
| `ultros-frontend/ultros-app/src/routes/item_view_verdicts.rs` (create) | `ItemVerdicts`, `SellVerdictCard`, `CraftVerdictCard`, helpers `sell_world`, `output_recipes`, `subcraft_recipes`, tests. |
| `ultros-frontend/ultros-app/src/routes/mod.rs` (modify) | `pub mod item_view_verdicts;` |
| `ultros-frontend/ultros-app/src/routes/item_view.rs` (modify) | Mount `ItemVerdicts` in `ListingsContent`'s `#overview`. |
| `ultros-frontend/ultros-app/src/routes/item_view_scope.rs:27` (modify) | Drop the `?lens=` mention. |
| `ultros-frontend/ultros-app/src/routes/item_view_sections.rs:3-4` (modify) | Drop the "Later lens work…" sentence. |

---

### Task 1: Sell-side calculations in `ultros-calc`

**Files:**
- Create: `ultros-frontend/ultros-calc/src/verdict.rs`
- Modify: `ultros-frontend/ultros-calc/src/lib.rs`

**Interfaces:**
- Consumes: `crate::analysis::{real_price, RealPriceBreakdown, RealPriceEstimate}` (existing; `real_price(&[(price, qty, hq)], vendor: Option<i32>)`, `RealPriceBreakdown { nq, hq }`, `.primary() -> Option<(bool, RealPriceEstimate)>`, `RealPriceEstimate { value: i32, .. }`).
- Produces (used by Task 4):
  - `pub struct ListingSample { pub world_id: i32, pub price_per_unit: i32, pub quantity: i32, pub hq: bool }`
  - `pub struct SaleSample { pub world_id: i32, pub price_per_item: i32, pub quantity: i32, pub hq: bool, pub sold_at: i64 }`
  - `pub fn headline_hq(sales: &[SaleSample], vendor_price: Option<i32>) -> bool`
  - `pub struct Wall { pub listings: u32, pub units: u32, pub low: i32, pub high: i32 }`
  - `pub struct Gap { pub price: i32, pub percent: f64 }`
  - `pub fn wall_and_gap(sorted: &[(i32, i32)]) -> Option<(Wall, Option<Gap>)>`
  - `pub enum SaleRate { UnitsPerDay(f64), TooFewSales }`
  - `pub fn sale_rate(sales: &[SaleSample], world_id: i32, hq: bool, now: i64) -> SaleRate`
  - `pub enum FloorWarning { UnderRealPrice { percent: f64 }, AboveRecentSales }`
  - `pub struct BoardVerdict { pub floor: i32, pub fast: i32, pub wall: Wall, pub gap: Option<Gap>, pub patient: Option<i32>, pub listed_units: u32, pub days_of_stock: Option<f64>, pub patient_eta_days: Option<f64>, pub warning: Option<FloorWarning> }`
  - `pub struct SellVerdict { pub hq: bool, pub real_price: Option<i32>, pub real_price_scope_wide: bool, pub rate: SaleRate, pub board: Option<BoardVerdict> }`
  - `pub fn sell_verdict(listings: &[ListingSample], sales: &[SaleSample], world_id: i32, vendor_price: Option<i32>, now: i64) -> SellVerdict`

- [ ] **Step 1: Register the module and write the failing tests**

In `ultros-frontend/ultros-calc/src/lib.rs`, add after `pub mod recipe_planner;`:

```rust
pub mod verdict;
```

Create `ultros-frontend/ultros-calc/src/verdict.rs` containing only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    const NOW: i64 = 1_800_000_000;
    const WORLD: i32 = 40;
    const OTHER: i32 = 41;

    fn listing(world_id: i32, price: i32, quantity: i32, hq: bool) -> ListingSample {
        ListingSample { world_id, price_per_unit: price, quantity, hq }
    }

    fn sale(world_id: i32, price: i32, quantity: i32, hq: bool, sold_at: i64) -> SaleSample {
        SaleSample { world_id, price_per_item: price, quantity, hq, sold_at }
    }

    #[test]
    fn headline_hq_prefers_the_quality_with_more_sales() {
        let sales = [
            sale(WORLD, 100, 1, true, NOW - 10),
            sale(WORLD, 100, 1, true, NOW - 20),
            sale(WORLD, 50, 1, false, NOW - 30),
        ];
        assert!(headline_hq(&sales, None));
    }

    #[test]
    fn headline_hq_breaks_ties_and_empties_to_nq() {
        let tie = [sale(WORLD, 100, 1, true, NOW), sale(WORLD, 50, 1, false, NOW)];
        assert!(!headline_hq(&tie, None));
        assert!(!headline_hq(&[], None));
    }

    #[test]
    fn wall_of_a_single_listing_has_no_gap() {
        let (wall, gap) = wall_and_gap(&[(500, 3)]).unwrap();
        assert_eq!(wall, Wall { listings: 1, units: 3, low: 500, high: 500 });
        assert_eq!(gap, None);
    }

    #[test]
    fn wall_spanning_the_whole_board_has_no_gap() {
        let (wall, gap) = wall_and_gap(&[(100, 1), (104, 2), (108, 5)]).unwrap();
        assert_eq!(wall, Wall { listings: 3, units: 8, low: 100, high: 108 });
        assert_eq!(gap, None);
    }

    #[test]
    fn a_step_of_exactly_five_percent_stays_in_the_wall() {
        let (wall, gap) = wall_and_gap(&[(100, 1), (105, 1), (111, 1)]).unwrap();
        assert_eq!(wall.high, 105);
        let gap = gap.unwrap();
        assert_eq!(gap.price, 111);
        assert!((gap.percent - 100.0 * 6.0 / 105.0).abs() < 1e-9);
    }

    #[test]
    fn empty_board_has_no_wall() {
        assert_eq!(wall_and_gap(&[]), None);
    }

    #[test]
    fn sale_rate_uses_only_the_world_and_quality() {
        let sales = [
            sale(WORLD, 100, 2, false, NOW - DAY),
            sale(WORLD, 100, 3, false, NOW - DAY),
            sale(WORLD, 100, 5, false, NOW - 2 * DAY),
            sale(WORLD, 100, 50, true, NOW - DAY),
            sale(OTHER, 100, 50, false, NOW - 2 * DAY),
        ];
        // Window = oldest sale (2 days) → 10 NQ units on WORLD over 2 days.
        match sale_rate(&sales, WORLD, false, NOW) {
            SaleRate::UnitsPerDay(rate) => assert!((rate - 5.0).abs() < 1e-9),
            other => panic!("expected a rate, got {other:?}"),
        }
    }

    #[test]
    fn sale_rate_window_is_capped_at_seven_days() {
        let sales = [
            sale(WORLD, 100, 7, false, NOW - DAY),
            sale(WORLD, 100, 7, false, NOW - 2 * DAY),
            sale(WORLD, 100, 7, false, NOW - 3 * DAY),
            sale(OTHER, 100, 1, false, NOW - 30 * DAY),
        ];
        match sale_rate(&sales, WORLD, false, NOW) {
            SaleRate::UnitsPerDay(rate) => assert!((rate - 3.0).abs() < 1e-9),
            other => panic!("expected a rate, got {other:?}"),
        }
    }

    #[test]
    fn sale_rate_needs_three_sales() {
        let sales = [
            sale(WORLD, 100, 9, false, NOW - DAY),
            sale(WORLD, 100, 9, false, NOW - DAY),
        ];
        assert_eq!(sale_rate(&sales, WORLD, false, NOW), SaleRate::TooFewSales);
        assert_eq!(sale_rate(&[], WORLD, false, NOW), SaleRate::TooFewSales);
    }

    #[test]
    fn sale_rate_ignores_sales_older_than_the_window() {
        let sales = [
            sale(WORLD, 100, 1, false, NOW - 20 * DAY),
            sale(WORLD, 100, 1, false, NOW - 21 * DAY),
            sale(WORLD, 100, 1, false, NOW - 22 * DAY),
        ];
        assert_eq!(sale_rate(&sales, WORLD, false, NOW), SaleRate::TooFewSales);
    }

    #[test]
    fn sale_rate_survives_sales_in_the_future() {
        let sales = [
            sale(WORLD, 100, 1, false, NOW + 60),
            sale(WORLD, 100, 1, false, NOW + 60),
            sale(WORLD, 100, 1, false, NOW + 60),
        ];
        match sale_rate(&sales, WORLD, false, NOW) {
            SaleRate::UnitsPerDay(rate) => assert!(rate.is_finite() && rate > 0.0),
            other => panic!("expected a rate, got {other:?}"),
        }
    }

    fn steady_sales(world_id: i32, hq: bool) -> Vec<SaleSample> {
        // 4 sales of 1 unit at 1000 over the last 2 days: rate 2 units/day.
        (0..4)
            .map(|i| sale(world_id, 1000, 1, hq, NOW - DAY / 2 - i * (DAY / 2)))
            .collect()
    }

    #[test]
    fn sell_verdict_builds_fast_and_patient_prices() {
        let listings = [
            listing(WORLD, 900, 2, false),
            listing(WORLD, 920, 3, false),
            listing(WORLD, 1100, 1, false),
            listing(WORLD, 10, 99, true),
            listing(OTHER, 1, 99, false),
        ];
        let v = sell_verdict(&listings, &steady_sales(WORLD, false), WORLD, None, NOW);
        assert!(!v.hq);
        assert_eq!(v.real_price, Some(1000));
        assert!(!v.real_price_scope_wide);
        let board = v.board.unwrap();
        assert_eq!(board.floor, 900);
        assert_eq!(board.fast, 899);
        assert_eq!(board.wall, Wall { listings: 2, units: 5, low: 900, high: 920 });
        assert_eq!(board.patient, Some(1099));
        assert_eq!(board.listed_units, 6);
        let rate = match v.rate {
            SaleRate::UnitsPerDay(rate) => rate,
            other => panic!("expected a rate, got {other:?}"),
        };
        assert!((board.days_of_stock.unwrap() - 6.0 / rate).abs() < 1e-9);
        assert!((board.patient_eta_days.unwrap() - 5.0 / rate).abs() < 1e-9);
        assert_eq!(board.warning, None);
    }

    #[test]
    fn fast_price_never_drops_below_one() {
        let v = sell_verdict(&[listing(WORLD, 1, 1, false)], &[], WORLD, None, NOW);
        assert_eq!(v.board.unwrap().fast, 1);
    }

    #[test]
    fn no_rate_means_no_day_estimates() {
        let v = sell_verdict(&[listing(WORLD, 900, 2, false), listing(WORLD, 2000, 1, false)], &[], WORLD, None, NOW);
        let board = v.board.unwrap();
        assert_eq!(v.rate, SaleRate::TooFewSales);
        assert_eq!(board.days_of_stock, None);
        assert_eq!(board.patient_eta_days, None);
    }

    #[test]
    fn real_price_falls_back_to_the_scope() {
        let v = sell_verdict(&[listing(WORLD, 900, 1, false)], &steady_sales(OTHER, false), WORLD, None, NOW);
        assert_eq!(v.real_price, Some(1000));
        assert!(v.real_price_scope_wide);
    }

    #[test]
    fn no_sales_anywhere_means_no_real_price() {
        let v = sell_verdict(&[listing(WORLD, 900, 1, false)], &[], WORLD, None, NOW);
        assert_eq!(v.real_price, None);
        assert!(!v.real_price_scope_wide);
        assert_eq!(v.board.unwrap().warning, None);
    }

    #[test]
    fn empty_board_for_the_headline_quality() {
        let v = sell_verdict(&[listing(WORLD, 900, 1, true)], &steady_sales(WORLD, false), WORLD, None, NOW);
        assert!(!v.hq);
        assert_eq!(v.board, None);
        assert_eq!(v.real_price, Some(1000));
    }

    fn warning_for(floor: i32) -> Option<FloorWarning> {
        sell_verdict(&[listing(WORLD, floor, 1, false)], &steady_sales(WORLD, false), WORLD, None, NOW)
            .board
            .unwrap()
            .warning
    }

    #[test]
    fn floor_warnings_trip_strictly_outside_the_band() {
        // Real price is 1000.
        assert_eq!(warning_for(700), None);
        match warning_for(699) {
            Some(FloorWarning::UnderRealPrice { percent }) => assert!((percent - 30.1).abs() < 1e-9),
            other => panic!("expected an under-real warning, got {other:?}"),
        }
        assert_eq!(warning_for(1500), None);
        assert_eq!(warning_for(1501), Some(FloorWarning::AboveRecentSales));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-calc verdict`
Expected: compile errors (`cannot find struct ListingSample`, `cannot find function sell_verdict`, …).

- [ ] **Step 3: Implement the sell side**

Insert above the `#[cfg(test)]` module in `verdict.rs`:

```rust
//! Item-page verdicts: what to list an item at, and whether to craft or buy it.
//!
//! Pure functions over the item page's listings and recent sales, so they can
//! be tested without Leptos. The thresholds are first guesses and live here so
//! tuning one is a one-line change.

use crate::analysis::{RealPriceBreakdown, RealPriceEstimate, real_price};

/// A wall keeps growing while each next listing is at most this multiple of
/// the one before it.
pub const WALL_STEP: f64 = 1.05;
/// Longest stretch of recent sales the sale rate looks back over, in days.
pub const RATE_WINDOW_DAYS: f64 = 7.0;
/// With fewer matching sales than this the rate is "too few sales".
pub const MIN_RATE_SALES: usize = 3;
/// A floor under this fraction of real price earns a warning.
pub const FLOOR_LOW: f64 = 0.7;
/// A floor over this multiple of real price earns a warning.
pub const FLOOR_HIGH: f64 = 1.5;
/// Craft and buy within this fraction of each other are "about even".
pub const EVEN_BAND: f64 = 0.03;

const SECONDS_PER_DAY: f64 = 86_400.0;
/// Shortest rate window, so a burst of same-second sales (or a client clock
/// behind the server) cannot divide by zero.
const MIN_WINDOW_DAYS: f64 = 1.0 / 24.0;

/// One listing on the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListingSample {
    pub world_id: i32,
    pub price_per_unit: i32,
    pub quantity: i32,
    pub hq: bool,
}

/// One recent sale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaleSample {
    pub world_id: i32,
    pub price_per_item: i32,
    pub quantity: i32,
    pub hq: bool,
    /// Unix seconds.
    pub sold_at: i64,
}

fn breakdown<'a>(
    sales: impl Iterator<Item = &'a SaleSample>,
    vendor_price: Option<i32>,
) -> RealPriceBreakdown {
    let samples: Vec<(i32, i32, bool)> = sales
        .map(|sale| (sale.price_per_item, sale.quantity, sale.hq))
        .collect();
    real_price(&samples, vendor_price)
}

fn estimate_for(breakdown: &RealPriceBreakdown, hq: bool) -> Option<RealPriceEstimate> {
    if hq { breakdown.hq } else { breakdown.nq }
}

/// The quality both verdict cards describe: `real_price`'s headline quality
/// over the page scope's sales (more sales wins, NQ on a tie), NQ when there
/// are no sales at all.
pub fn headline_hq(sales: &[SaleSample], vendor_price: Option<i32>) -> bool {
    breakdown(sales.iter(), vendor_price)
        .primary()
        .is_some_and(|(hq, _)| hq)
}

/// The cheapest run of listings: from the floor, while each next price is at
/// most [`WALL_STEP`] times the previous one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wall {
    pub listings: u32,
    pub units: u32,
    pub low: i32,
    pub high: i32,
}

/// The first listing above the wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap {
    pub price: i32,
    /// `(price - wall.high) / wall.high`, in percent.
    pub percent: f64,
}

fn units(quantity: i32) -> u32 {
    quantity.max(0) as u32
}

/// The wall and the gap above it, from `(price, quantity)` pairs sorted by
/// price ascending. `None` for an empty board.
pub fn wall_and_gap(sorted: &[(i32, i32)]) -> Option<(Wall, Option<Gap>)> {
    let (&(low, quantity), rest) = sorted.split_first()?;
    let mut wall = Wall {
        listings: 1,
        units: units(quantity),
        low,
        high: low,
    };
    for &(price, quantity) in rest {
        if f64::from(price) <= f64::from(wall.high) * WALL_STEP {
            wall.listings += 1;
            wall.units += units(quantity);
            wall.high = price;
        } else {
            let percent =
                f64::from(price - wall.high) / f64::from(wall.high.max(1)) * 100.0;
            return Some((wall, Some(Gap { price, percent })));
        }
    }
    Some((wall, None))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SaleRate {
    UnitsPerDay(f64),
    TooFewSales,
}

/// Units of one quality sold per day on one world.
///
/// `sales` is the page scope's recent-sale buffer. The window runs from its
/// oldest sale to `now`, capped at [`RATE_WINDOW_DAYS`]: the buffer is shared
/// by every world in scope, so when a busy datacenter's buffer only reaches
/// back two days, two days is all that is known on any of its worlds.
pub fn sale_rate(sales: &[SaleSample], world_id: i32, hq: bool, now: i64) -> SaleRate {
    let Some(oldest) = sales.iter().map(|sale| sale.sold_at).min() else {
        return SaleRate::TooFewSales;
    };
    let window_days =
        ((now - oldest) as f64 / SECONDS_PER_DAY).clamp(MIN_WINDOW_DAYS, RATE_WINDOW_DAYS);
    let cutoff = now - (window_days * SECONDS_PER_DAY) as i64;
    let (count, sold) = sales
        .iter()
        .filter(|sale| sale.world_id == world_id && sale.hq == hq && sale.sold_at >= cutoff)
        .fold((0usize, 0u64), |(count, sold), sale| {
            (count + 1, sold + u64::from(units(sale.quantity)))
        });
    if count < MIN_RATE_SALES {
        return SaleRate::TooFewSales;
    }
    SaleRate::UnitsPerDay(sold as f64 / window_days)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FloorWarning {
    /// The floor sits this many percent under real price.
    UnderRealPrice { percent: f64 },
    AboveRecentSales,
}

fn floor_warning(floor: i32, real: i32) -> Option<FloorWarning> {
    if real <= 0 {
        return None;
    }
    let (floor, real) = (f64::from(floor), f64::from(real));
    if floor < real * FLOOR_LOW {
        Some(FloorWarning::UnderRealPrice {
            percent: (real - floor) / real * 100.0,
        })
    } else if floor > real * FLOOR_HIGH {
        Some(FloorWarning::AboveRecentSales)
    } else {
        None
    }
}

/// What the board says about listing now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardVerdict {
    pub floor: i32,
    /// One gil under the floor (never below 1).
    pub fast: i32,
    pub wall: Wall,
    pub gap: Option<Gap>,
    /// One gil under the gap listing; `None` without a gap.
    pub patient: Option<i32>,
    /// Units listed at the headline quality on the world.
    pub listed_units: u32,
    pub days_of_stock: Option<f64>,
    /// Days until the wall ahead of a patient listing sells through.
    pub patient_eta_days: Option<f64>,
    pub warning: Option<FloorWarning>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SellVerdict {
    pub hq: bool,
    pub real_price: Option<i32>,
    /// The world had no sales at `hq`, so `real_price` is the page scope's.
    pub real_price_scope_wide: bool,
    pub rate: SaleRate,
    /// `None` when nothing is listed at `hq` on the world.
    pub board: Option<BoardVerdict>,
}

/// The sell verdict for one world.
///
/// `listings` may span the page scope; only `world_id`'s rows are used.
/// `sales` is the page scope's recent-sale buffer.
pub fn sell_verdict(
    listings: &[ListingSample],
    sales: &[SaleSample],
    world_id: i32,
    vendor_price: Option<i32>,
    now: i64,
) -> SellVerdict {
    let hq = headline_hq(sales, vendor_price);
    let world_real = estimate_for(
        &breakdown(sales.iter().filter(|sale| sale.world_id == world_id), vendor_price),
        hq,
    );
    let (real_price, real_price_scope_wide) = match world_real {
        Some(estimate) => (Some(estimate.value), false),
        None => {
            let scope = estimate_for(&breakdown(sales.iter(), vendor_price), hq)
                .map(|estimate| estimate.value);
            (scope, scope.is_some())
        }
    };
    let rate = sale_rate(sales, world_id, hq, now);
    let per_day = match rate {
        SaleRate::UnitsPerDay(rate) if rate > 0.0 => Some(rate),
        _ => None,
    };

    let mut board: Vec<(i32, i32)> = listings
        .iter()
        .filter(|listing| listing.world_id == world_id && listing.hq == hq)
        .map(|listing| (listing.price_per_unit, listing.quantity))
        .collect();
    board.sort_unstable();
    let listed_units: u32 = board.iter().map(|&(_, quantity)| units(quantity)).sum();

    let board = wall_and_gap(&board).map(|(wall, gap)| BoardVerdict {
        floor: wall.low,
        fast: (wall.low - 1).max(1),
        wall,
        gap,
        patient: gap.map(|gap| (gap.price - 1).max(1)),
        listed_units,
        days_of_stock: per_day.map(|rate| f64::from(listed_units) / rate),
        patient_eta_days: gap.and(per_day).map(|rate| f64::from(wall.units) / rate),
        warning: real_price.and_then(|real| floor_warning(wall.low, real)),
    });

    SellVerdict {
        hq,
        real_price,
        real_price_scope_wide,
        rate,
        board,
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ultros-calc verdict`
Expected: all 18 tests PASS.

If `sell_verdict_builds_fast_and_patient_prices` fails on `real_price`, print `real_price(&samples, None)` for four 1000-gil sales: `real_price` takes the IQR-filtered mean for ≥ 4 samples, which is 1000 here. Do not change `analysis.rs`.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
git add ultros-frontend/ultros-calc/src/verdict.rs ultros-frontend/ultros-calc/src/lib.rs
git commit -m "feat(calc): sell verdict — wall, gap, sale rate, days of stock

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Expected: `REAL_EXIT=0`.

---

### Task 2: Craft-side calculations in `ultros-calc`

**Files:**
- Modify: `ultros-frontend/ultros-calc/src/verdict.rs`

**Interfaces:**
- Consumes: `EVEN_BAND` from Task 1.
- Produces (used by Task 5):
  - `pub enum BuySource { Market { nq_fallback: bool }, Vendor }`
  - `pub struct BuyPrice { pub price: i32, pub source: BuySource }`
  - `pub fn buy_price(hq: bool, market_hq: Option<i32>, market_nq: Option<i32>, vendor: Option<i32>) -> Option<BuyPrice>`
  - `pub fn craft_unit_cost(cost: i32, amount_result: i32) -> i32`
  - `pub enum CraftVerdict { CraftSaves { gil: i32, percent: f64 }, BuyCheaper { gil: i32, percent: f64 }, AboutEven, Incomplete }`
  - `pub fn craft_verdict(craft_unit: i32, buy: Option<i32>, unpriced_lines: u16) -> CraftVerdict`

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `verdict.rs`:

```rust
    #[test]
    fn craft_unit_cost_divides_by_yield() {
        assert_eq!(craft_unit_cost(300, 3), 100);
        assert_eq!(craft_unit_cost(300, 1), 300);
        assert_eq!(craft_unit_cost(300, 0), 300);
    }

    #[test]
    fn craft_verdict_band_edges() {
        match craft_verdict(969, Some(1000), 0) {
            CraftVerdict::CraftSaves { gil, percent } => {
                assert_eq!(gil, 31);
                assert!((percent - 3.1).abs() < 1e-9);
            }
            other => panic!("expected craft saves, got {other:?}"),
        }
        assert_eq!(craft_verdict(971, Some(1000), 0), CraftVerdict::AboutEven);
        assert_eq!(craft_verdict(1029, Some(1000), 0), CraftVerdict::AboutEven);
        match craft_verdict(1031, Some(1000), 0) {
            CraftVerdict::BuyCheaper { gil, percent } => {
                assert_eq!(gil, 31);
                assert!((percent - 100.0 * 31.0 / 1031.0).abs() < 1e-9);
            }
            other => panic!("expected buy cheaper, got {other:?}"),
        }
    }

    #[test]
    fn craft_verdict_is_incomplete_without_prices() {
        assert_eq!(craft_verdict(500, Some(1000), 1), CraftVerdict::Incomplete);
        assert_eq!(craft_verdict(500, None, 0), CraftVerdict::Incomplete);
        assert_eq!(craft_verdict(500, Some(0), 0), CraftVerdict::Incomplete);
    }

    #[test]
    fn craft_verdict_zero_cost_from_on_hand_saves_everything() {
        match craft_verdict(0, Some(100), 0) {
            CraftVerdict::CraftSaves { gil, percent } => {
                assert_eq!(gil, 100);
                assert!((percent - 100.0).abs() < 1e-9);
            }
            other => panic!("expected craft saves, got {other:?}"),
        }
    }

    #[test]
    fn buy_price_takes_the_cheaper_vendor_for_nq() {
        assert_eq!(
            buy_price(false, None, Some(200), Some(68)),
            Some(BuyPrice { price: 68, source: BuySource::Vendor })
        );
        assert_eq!(
            buy_price(false, None, Some(50), Some(68)),
            Some(BuyPrice { price: 50, source: BuySource::Market { nq_fallback: false } })
        );
        assert_eq!(
            buy_price(false, None, None, Some(68)),
            Some(BuyPrice { price: 68, source: BuySource::Vendor })
        );
    }

    #[test]
    fn buy_price_ignores_the_vendor_for_hq() {
        assert_eq!(
            buy_price(true, Some(300), Some(200), Some(68)),
            Some(BuyPrice { price: 300, source: BuySource::Market { nq_fallback: false } })
        );
        assert_eq!(buy_price(true, None, None, Some(68)), None);
    }

    #[test]
    fn buy_price_falls_back_to_nq_for_hq() {
        assert_eq!(
            buy_price(true, None, Some(200), None),
            Some(BuyPrice { price: 200, source: BuySource::Market { nq_fallback: true } })
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-calc verdict`
Expected: compile errors (`cannot find function craft_verdict`, …).

- [ ] **Step 3: Implement the craft side**

Insert above `#[cfg(test)]` in `verdict.rs`:

```rust
/// Where the "buy" side of the craft verdict comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuySource {
    /// Cheapest listing in the viewer's price zone. `nq_fallback`: an HQ
    /// verdict with no HQ listing, priced from the NQ one.
    Market { nq_fallback: bool },
    /// An NPC gil shop.
    Vendor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuyPrice {
    pub price: i32,
    pub source: BuySource,
}

/// What buying one unit at the headline quality costs. Vendors only sell NQ,
/// so `vendor` is considered for an NQ verdict only.
pub fn buy_price(
    hq: bool,
    market_hq: Option<i32>,
    market_nq: Option<i32>,
    vendor: Option<i32>,
) -> Option<BuyPrice> {
    let market = |price, nq_fallback| BuyPrice {
        price,
        source: BuySource::Market { nq_fallback },
    };
    let market = if hq {
        market_hq
            .map(|price| market(price, false))
            .or_else(|| market_nq.map(|price| market(price, true)))
    } else {
        market_nq.map(|price| market(price, false))
    };
    let vendor = vendor
        .filter(|price| !hq && *price > 0)
        .map(|price| BuyPrice {
            price,
            source: BuySource::Vendor,
        });
    match (market, vendor) {
        (Some(market), Some(vendor)) => Some(if vendor.price < market.price {
            vendor
        } else {
            market
        }),
        (market, vendor) => market.or(vendor),
    }
}

/// `compute_cost` prices one execution of a recipe; this is the cost of one
/// of the `amount_result` units it yields.
pub fn craft_unit_cost(cost: i32, amount_result: i32) -> i32 {
    cost / amount_result.max(1)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CraftVerdict {
    CraftSaves { gil: i32, percent: f64 },
    BuyCheaper { gil: i32, percent: f64 },
    AboutEven,
    /// An ingredient had no listing, or there is nothing to buy to compare with.
    Incomplete,
}

/// Compare one crafted unit with one bought unit. `unpriced_lines` is
/// `CostBreakdown::unpriced_market_lines`.
pub fn craft_verdict(craft_unit: i32, buy: Option<i32>, unpriced_lines: u16) -> CraftVerdict {
    let Some(buy) = buy.filter(|price| *price > 0) else {
        return CraftVerdict::Incomplete;
    };
    if unpriced_lines > 0 {
        return CraftVerdict::Incomplete;
    }
    let (craft, bought) = (f64::from(craft_unit), f64::from(buy));
    if craft < bought * (1.0 - EVEN_BAND) {
        CraftVerdict::CraftSaves {
            gil: buy - craft_unit,
            percent: (bought - craft) / bought * 100.0,
        }
    } else if craft > bought * (1.0 + EVEN_BAND) {
        CraftVerdict::BuyCheaper {
            gil: craft_unit - buy,
            percent: (craft - bought) / craft * 100.0,
        }
    } else {
        CraftVerdict::AboutEven
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ultros-calc verdict`
Expected: all 25 tests PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
git add ultros-frontend/ultros-calc/src/verdict.rs
git commit -m "feat(calc): craft-or-buy verdict with per-unit cost and vendor buy price

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: i18n keys in all seven locales

**Files:**
- Modify: `ultros-frontend/ultros-i18n/locales/en.json`, `fr.json`, `de.json`, `ja.json`, `cn.json`, `ko.json`, `tc.json`

**Interfaces:**
- Produces (used by Tasks 4–5): keys `item_verdict_region_label`, `item_verdict_sell_heading{world}`, `item_verdict_sell_pick_world`, `item_verdict_fast`, `item_verdict_fast_hint`, `item_verdict_patient`, `item_verdict_patient_hint{units}`, `item_verdict_eta_days{days}`, `item_verdict_wall`, `item_verdict_wall_counts{listings,units}`, `item_verdict_gap{percent}`, `item_verdict_days_of_stock{days}`, `item_verdict_too_few_sales`, `item_verdict_no_recent_sales`, `item_verdict_no_listings{quality,world}`, `item_verdict_real_price_across{scope}`, `item_verdict_floor_under_real{percent}`, `item_verdict_floor_above_real`, `item_verdict_craft_heading`, `item_verdict_craft_unit`, `item_verdict_buy_zone{zone}`, `item_verdict_buy_vendor`, `item_verdict_craft_saves`, `item_verdict_buy_cheaper`, `item_verdict_about_even`, `item_verdict_incomplete`, `item_verdict_incl_subcrafts`, `item_verdict_crystals_excluded`, `item_verdict_recipe_link`. Reused existing keys: `hq`, `nq`, `real_price`, `no_data`.

- [ ] **Step 1: Insert the keys**

In each locale file, insert the block for that locale on the line directly after the existing `"item_view_savings_save": …,` line (every locale has it exactly once). Each block ends with a comma because more keys follow. Keep the files' 4-space indentation.

`en.json`:

```json
    "item_verdict_region_label": "Market verdicts",
    "item_verdict_sell_heading": "Sell on {{world}}",
    "item_verdict_sell_pick_world": "Pick a world to see where to list",
    "item_verdict_fast": "Fast",
    "item_verdict_fast_hint": "undercut the floor",
    "item_verdict_patient": "Patient",
    "item_verdict_patient_hint": "units ahead: {{units}}",
    "item_verdict_eta_days": "~{{days}} d",
    "item_verdict_wall": "Wall",
    "item_verdict_wall_counts": "listings {{listings}} · units {{units}}",
    "item_verdict_gap": "Gap above: +{{percent}}%",
    "item_verdict_days_of_stock": "Days of stock: ~{{days}}",
    "item_verdict_too_few_sales": "Too few sales to estimate stock",
    "item_verdict_no_recent_sales": "No recent sales",
    "item_verdict_no_listings": "No {{quality}} listings on {{world}}",
    "item_verdict_real_price_across": "across {{scope}}",
    "item_verdict_floor_under_real": "Floor is {{percent}}% under real price",
    "item_verdict_floor_above_real": "Floor is far above recent sales",
    "item_verdict_craft_heading": "Craft or buy",
    "item_verdict_craft_unit": "Craft / unit",
    "item_verdict_buy_zone": "Buy · cheapest in {{zone}}",
    "item_verdict_buy_vendor": "Buy · vendor",
    "item_verdict_craft_saves": "Craft saves",
    "item_verdict_buy_cheaper": "Buying is cheaper by",
    "item_verdict_about_even": "About even",
    "item_verdict_incomplete": "Some ingredients have no listings",
    "item_verdict_incl_subcrafts": "incl. sub-crafts",
    "item_verdict_crystals_excluded": "crystals excluded",
    "item_verdict_recipe_link": "See recipe breakdown",
```

`fr.json`:

```json
    "item_verdict_region_label": "Verdicts du marché",
    "item_verdict_sell_heading": "Vendre sur {{world}}",
    "item_verdict_sell_pick_world": "Choisissez un monde pour savoir à quel prix vendre",
    "item_verdict_fast": "Rapide",
    "item_verdict_fast_hint": "sous le prix plancher",
    "item_verdict_patient": "Patient",
    "item_verdict_patient_hint": "unités devant : {{units}}",
    "item_verdict_eta_days": "~{{days}} j",
    "item_verdict_wall": "Mur de prix",
    "item_verdict_wall_counts": "annonces {{listings}} · unités {{units}}",
    "item_verdict_gap": "Écart au-dessus : +{{percent}} %",
    "item_verdict_days_of_stock": "Jours de stock : ~{{days}}",
    "item_verdict_too_few_sales": "Trop peu de ventes pour estimer le stock",
    "item_verdict_no_recent_sales": "Aucune vente récente",
    "item_verdict_no_listings": "Aucune annonce {{quality}} sur {{world}}",
    "item_verdict_real_price_across": "sur {{scope}}",
    "item_verdict_floor_under_real": "Le prix plancher est {{percent}} % sous le prix réel",
    "item_verdict_floor_above_real": "Le prix plancher dépasse largement les ventes récentes",
    "item_verdict_craft_heading": "Fabriquer ou acheter",
    "item_verdict_craft_unit": "Fabrication / unité",
    "item_verdict_buy_zone": "Achat · le moins cher sur {{zone}}",
    "item_verdict_buy_vendor": "Achat · marchand",
    "item_verdict_craft_saves": "Fabriquer économise",
    "item_verdict_buy_cheaper": "Acheter est moins cher de",
    "item_verdict_about_even": "À peu près égal",
    "item_verdict_incomplete": "Certains ingrédients n'ont aucune annonce",
    "item_verdict_incl_subcrafts": "sous-fabrications incluses",
    "item_verdict_crystals_excluded": "cristaux exclus",
    "item_verdict_recipe_link": "Voir le détail de la recette",
```

`de.json`:

```json
    "item_verdict_region_label": "Markteinschätzung",
    "item_verdict_sell_heading": "Verkaufen auf {{world}}",
    "item_verdict_sell_pick_world": "Wähle eine Welt, um einen Verkaufspreis zu sehen",
    "item_verdict_fast": "Schnell",
    "item_verdict_fast_hint": "Mindestpreis unterbieten",
    "item_verdict_patient": "Geduldig",
    "item_verdict_patient_hint": "Einheiten davor: {{units}}",
    "item_verdict_eta_days": "~{{days}} T.",
    "item_verdict_wall": "Preiswand",
    "item_verdict_wall_counts": "Angebote {{listings}} · Einheiten {{units}}",
    "item_verdict_gap": "Abstand darüber: +{{percent}} %",
    "item_verdict_days_of_stock": "Vorrat in Tagen: ~{{days}}",
    "item_verdict_too_few_sales": "Zu wenige Verkäufe für eine Schätzung",
    "item_verdict_no_recent_sales": "Keine aktuellen Verkäufe",
    "item_verdict_no_listings": "Keine {{quality}}-Angebote auf {{world}}",
    "item_verdict_real_price_across": "über {{scope}}",
    "item_verdict_floor_under_real": "Mindestpreis liegt {{percent}} % unter dem realen Preis",
    "item_verdict_floor_above_real": "Mindestpreis liegt weit über den letzten Verkäufen",
    "item_verdict_craft_heading": "Herstellen oder kaufen",
    "item_verdict_craft_unit": "Herstellung / Stück",
    "item_verdict_buy_zone": "Kauf · günstigstes in {{zone}}",
    "item_verdict_buy_vendor": "Kauf · Händler",
    "item_verdict_craft_saves": "Herstellen spart",
    "item_verdict_buy_cheaper": "Kaufen ist günstiger um",
    "item_verdict_about_even": "Etwa gleich",
    "item_verdict_incomplete": "Für einige Zutaten gibt es keine Angebote",
    "item_verdict_incl_subcrafts": "inkl. Zwischenprodukte",
    "item_verdict_crystals_excluded": "Kristalle ausgenommen",
    "item_verdict_recipe_link": "Rezeptdetails ansehen",
```

`ja.json`:

```json
    "item_verdict_region_label": "マーケット判定",
    "item_verdict_sell_heading": "{{world}}で出品",
    "item_verdict_sell_pick_world": "出品価格を見るにはワールドを選択してください",
    "item_verdict_fast": "即売り",
    "item_verdict_fast_hint": "最安値を下回る",
    "item_verdict_patient": "待ち",
    "item_verdict_patient_hint": "先行: {{units}}個",
    "item_verdict_eta_days": "約{{days}}日",
    "item_verdict_wall": "価格の壁",
    "item_verdict_wall_counts": "出品 {{listings}} · 個数 {{units}}",
    "item_verdict_gap": "上の価格差: +{{percent}}%",
    "item_verdict_days_of_stock": "在庫日数: 約{{days}}日",
    "item_verdict_too_few_sales": "販売数が少なく在庫を推定できません",
    "item_verdict_no_recent_sales": "最近の販売なし",
    "item_verdict_no_listings": "{{world}}に{{quality}}の出品なし",
    "item_verdict_real_price_across": "{{scope}}全体",
    "item_verdict_floor_under_real": "最安値が実勢価格より{{percent}}%安い",
    "item_verdict_floor_above_real": "最安値が最近の販売価格を大きく上回る",
    "item_verdict_craft_heading": "製作か購入か",
    "item_verdict_craft_unit": "製作 / 1個",
    "item_verdict_buy_zone": "購入 · {{zone}}の最安値",
    "item_verdict_buy_vendor": "購入 · NPC販売",
    "item_verdict_craft_saves": "製作で節約",
    "item_verdict_buy_cheaper": "購入の方が安い",
    "item_verdict_about_even": "ほぼ同じ",
    "item_verdict_incomplete": "一部の素材に出品がありません",
    "item_verdict_incl_subcrafts": "中間素材の製作を含む",
    "item_verdict_crystals_excluded": "クリスタル除外",
    "item_verdict_recipe_link": "レシピの内訳を見る",
```

`cn.json`:

```json
    "item_verdict_region_label": "市场判断",
    "item_verdict_sell_heading": "在{{world}}出售",
    "item_verdict_sell_pick_world": "选择一个服务器以查看挂售价格",
    "item_verdict_fast": "快速",
    "item_verdict_fast_hint": "压低最低价",
    "item_verdict_patient": "耐心",
    "item_verdict_patient_hint": "前方数量：{{units}}",
    "item_verdict_eta_days": "约{{days}}天",
    "item_verdict_wall": "价格墙",
    "item_verdict_wall_counts": "挂单 {{listings}} · 数量 {{units}}",
    "item_verdict_gap": "上方价差：+{{percent}}%",
    "item_verdict_days_of_stock": "库存天数：约{{days}}",
    "item_verdict_too_few_sales": "销售太少，无法估算库存",
    "item_verdict_no_recent_sales": "近期无销售",
    "item_verdict_no_listings": "{{world}}没有{{quality}}挂单",
    "item_verdict_real_price_across": "{{scope}}整体",
    "item_verdict_floor_under_real": "最低价比实际价格低{{percent}}%",
    "item_verdict_floor_above_real": "最低价远高于近期成交价",
    "item_verdict_craft_heading": "制作还是购买",
    "item_verdict_craft_unit": "制作 / 每个",
    "item_verdict_buy_zone": "购买 · {{zone}}最低价",
    "item_verdict_buy_vendor": "购买 · NPC商店",
    "item_verdict_craft_saves": "制作可节省",
    "item_verdict_buy_cheaper": "购买更便宜",
    "item_verdict_about_even": "大致相当",
    "item_verdict_incomplete": "部分材料没有挂单",
    "item_verdict_incl_subcrafts": "含半成品制作",
    "item_verdict_crystals_excluded": "不含水晶",
    "item_verdict_recipe_link": "查看配方明细",
```

`ko.json`:

```json
    "item_verdict_region_label": "시장 판단",
    "item_verdict_sell_heading": "{{world}}에서 판매",
    "item_verdict_sell_pick_world": "판매 가격을 보려면 월드를 선택하세요",
    "item_verdict_fast": "빠르게",
    "item_verdict_fast_hint": "최저가보다 낮게",
    "item_verdict_patient": "느긋하게",
    "item_verdict_patient_hint": "앞선 수량: {{units}}",
    "item_verdict_eta_days": "약 {{days}}일",
    "item_verdict_wall": "가격 벽",
    "item_verdict_wall_counts": "매물 {{listings}} · 수량 {{units}}",
    "item_verdict_gap": "위 가격 차이: +{{percent}}%",
    "item_verdict_days_of_stock": "재고 일수: 약 {{days}}일",
    "item_verdict_too_few_sales": "판매가 적어 재고를 추정할 수 없습니다",
    "item_verdict_no_recent_sales": "최근 판매 없음",
    "item_verdict_no_listings": "{{world}}에 {{quality}} 매물 없음",
    "item_verdict_real_price_across": "{{scope}} 전체",
    "item_verdict_floor_under_real": "최저가가 실거래가보다 {{percent}}% 낮음",
    "item_verdict_floor_above_real": "최저가가 최근 판매가보다 훨씬 높음",
    "item_verdict_craft_heading": "제작 또는 구매",
    "item_verdict_craft_unit": "제작 / 개당",
    "item_verdict_buy_zone": "구매 · {{zone}} 최저가",
    "item_verdict_buy_vendor": "구매 · NPC 상점",
    "item_verdict_craft_saves": "제작 시 절약",
    "item_verdict_buy_cheaper": "구매가 더 저렴함",
    "item_verdict_about_even": "거의 같음",
    "item_verdict_incomplete": "일부 재료에 매물이 없습니다",
    "item_verdict_incl_subcrafts": "중간 재료 제작 포함",
    "item_verdict_crystals_excluded": "크리스탈 제외",
    "item_verdict_recipe_link": "레시피 내역 보기",
```

`tc.json`:

```json
    "item_verdict_region_label": "市場判斷",
    "item_verdict_sell_heading": "在{{world}}出售",
    "item_verdict_sell_pick_world": "選擇一個伺服器以查看掛售價格",
    "item_verdict_fast": "快速",
    "item_verdict_fast_hint": "壓低最低價",
    "item_verdict_patient": "耐心",
    "item_verdict_patient_hint": "前方數量：{{units}}",
    "item_verdict_eta_days": "約{{days}}天",
    "item_verdict_wall": "價格牆",
    "item_verdict_wall_counts": "掛單 {{listings}} · 數量 {{units}}",
    "item_verdict_gap": "上方價差：+{{percent}}%",
    "item_verdict_days_of_stock": "庫存天數：約{{days}}",
    "item_verdict_too_few_sales": "銷售太少，無法估算庫存",
    "item_verdict_no_recent_sales": "近期無銷售",
    "item_verdict_no_listings": "{{world}}沒有{{quality}}掛單",
    "item_verdict_real_price_across": "{{scope}}整體",
    "item_verdict_floor_under_real": "最低價比實際價格低{{percent}}%",
    "item_verdict_floor_above_real": "最低價遠高於近期成交價",
    "item_verdict_craft_heading": "製作還是購買",
    "item_verdict_craft_unit": "製作 / 每個",
    "item_verdict_buy_zone": "購買 · {{zone}}最低價",
    "item_verdict_buy_vendor": "購買 · NPC商店",
    "item_verdict_craft_saves": "製作可節省",
    "item_verdict_buy_cheaper": "購買更便宜",
    "item_verdict_about_even": "大致相當",
    "item_verdict_incomplete": "部分材料沒有掛單",
    "item_verdict_incl_subcrafts": "含半成品製作",
    "item_verdict_crystals_excluded": "不含水晶",
    "item_verdict_recipe_link": "查看配方明細",
```

- [ ] **Step 2: Verify every file is valid JSON with the same 29 new keys**

Run:

```bash
for f in ultros-frontend/ultros-i18n/locales/*.json; do
  python -c "import json,sys; d=json.load(open(sys.argv[1],encoding='utf-8')); k=[x for x in d if x.startswith('item_verdict_')]; print(sys.argv[1], len(k))" "$f"
done
```

Expected: every file prints `29`.

- [ ] **Step 3: Verify the i18n crate still builds**

Run: `cargo check -p ultros-i18n`
Expected: finishes without errors or missing-key warnings.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all -- --check
git add ultros-frontend/ultros-i18n/locales/
git commit -m "i18n: item page verdict strings in all locales

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Sell verdict card, mounted in `#overview`

**Files:**
- Create: `ultros-frontend/ultros-app/src/routes/item_view_verdicts.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/mod.rs` (next to `pub mod item_view_sections;`)
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` (`ListingsContent`'s `#overview` block, around line 1472)
- Modify: `ultros-frontend/ultros-app/src/routes/item_view_scope.rs:25-27`
- Modify: `ultros-frontend/ultros-app/src/routes/item_view_sections.rs:1-5`

**Interfaces:**
- Consumes: Task 1's `ListingSample`, `SaleSample`, `sell_verdict`, `SellVerdict`, `BoardVerdict`, `SaleRate`, `FloorWarning`; Task 3's keys; existing `crate::routes::item_view::{with_or, get_or_default}` (both `pub(crate)`), `use_home_world() -> (Signal<Option<World>>, _)`, `LocalWorldData(pub AppResult<Arc<WorldHelper>>)`, `WorldHelper::lookup_world_by_name(&str) -> Option<AnyResult>`, `AnyResult::{as_world, all_worlds}`, `WorldHelper::lookup_selector(AnySelector) -> Option<AnyResult>`.
- Produces (used by Task 5):
  - `pub(crate) fn ItemVerdicts(listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>, filtered_listings: Signal<ListingRows>, excluded_worlds: Signal<HashSet<i32>>, world: Memo<String>, item_id: Memo<i32>)` component
  - `const CARD_CLASS: &str`, `fn quality_chip(hq: bool) -> AnyView`, `fn sale_samples(sales: &[SaleHistory]) -> Vec<SaleSample>`, `fn laundering_vendor_price(item_id: i32) -> Option<i32>`, `pub(crate) fn output_recipes(item_id: i32) -> Vec<&'static Recipe>`

- [ ] **Step 1: Write the failing tests and module skeleton**

Add to `ultros-frontend/ultros-app/src/routes/mod.rs` after `pub mod item_view_sections;`:

```rust
pub mod item_view_verdicts;
```

Create `ultros-frontend/ultros-app/src/routes/item_view_verdicts.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sell_world_prefers_the_page_world() {
        assert_eq!(sell_world(Some(40), &[40], &HashSet::new(), Some(41)), Some(40));
    }

    #[test]
    fn sell_world_uses_the_home_world_inside_the_scope() {
        assert_eq!(sell_world(None, &[40, 41, 42], &HashSet::new(), Some(41)), Some(41));
    }

    #[test]
    fn sell_world_is_none_when_home_is_outside_the_scope_or_unset() {
        assert_eq!(sell_world(None, &[40, 41], &HashSet::new(), Some(99)), None);
        assert_eq!(sell_world(None, &[40, 41], &HashSet::new(), None), None);
    }

    #[test]
    fn sell_world_ignores_excluded_worlds() {
        let excluded = HashSet::from([41]);
        assert_eq!(sell_world(None, &[40, 41], &excluded, Some(41)), None);
        assert_eq!(sell_world(Some(41), &[41], &excluded, None), None);
    }

    #[test]
    fn output_recipes_finds_the_recipes_that_make_an_item() {
        let recipe = tracked_data()
            .recipes
            .values()
            .find(|recipe| recipe.item_result > 0)
            .expect("game data has recipes");
        let found = output_recipes(recipe.item_result);
        assert!(found.iter().any(|r| r.key_id == recipe.key_id));
        assert!(found.iter().all(|r| r.item_result == recipe.item_result));
    }

    /// Mirrors `real_price_summary_builds_without_an_i18n_context` in
    /// `item_view.rs`: the cards read i18n and every context through
    /// non-panicking accessors, so building them in a bare owner (no i18n,
    /// no cookies, no world data) must not panic.
    #[test]
    fn item_verdicts_builds_without_contexts() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let listing_resource =
                Resource::new(|| (), |_| async { Err(AppError::ParamMissing) });
            let filtered_listings: Signal<Vec<(ActiveListing, Arc<Retainer>)>> =
                Signal::derive(Vec::new);
            let excluded_worlds: Signal<HashSet<i32>> = Signal::derive(HashSet::new);
            let world = Memo::new(|_| "Gilgamesh".to_string());
            let item_id = Memo::new(|_| 5057);
            let _ = view! {
                <ItemVerdicts listing_resource filtered_listings excluded_worlds world item_id />
            };
        });
    }
}
```

Run: `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros-app --lib item_view_verdicts`
Expected: compile errors (`cannot find function sell_world`, `ItemVerdicts`).

- [ ] **Step 2: Implement helpers, `ItemVerdicts` and `SellVerdictCard`**

Put this above the test module in `item_view_verdicts.rs`:

```rust
//! Sell and craft verdicts at the top of the item page.
//!
//! The arithmetic lives in `ultros_calc::verdict`; this module adapts the
//! page's listings payload and contexts into it and renders two compact
//! cards inside `#overview`.

use std::collections::HashSet;
use std::sync::Arc;

use leptos::prelude::*;
use leptos_router::location::Url;
use ultros_api_types::world_helper::AnySelector;
use ultros_api_types::{ActiveListing, CurrentlyShownItem, Retainer, SaleHistory};
use ultros_calc::verdict::{
    BoardVerdict, FloorWarning, ListingSample, SaleRate, SaleSample, SellVerdict, sell_verdict,
};
use xiv_gen::{ItemId, Recipe};

use crate::components::gil::Gil;
use crate::components::skeleton::SingleLineSkeleton;
use crate::error::AppError;
use crate::global_state::LocalWorldData;
use crate::global_state::cookies::Cookies;
use crate::global_state::home_world::use_home_world;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string};
use crate::routes::item_view::{get_or_default, with_or};

type ListingRows = Vec<(ActiveListing, Arc<Retainer>)>;

const CARD_CLASS: &str = "item-surface flex min-w-0 flex-col gap-1.5 p-3 text-sm";
const MUTED: &str = "text-[color:var(--color-text-muted)]";

/// The world whose board the sell card describes: the page's world, else the
/// home world when it is in scope. An excluded world is never used.
pub(crate) fn sell_world(
    page_world: Option<i32>,
    scope_worlds: &[i32],
    excluded: &HashSet<i32>,
    home_world: Option<i32>,
) -> Option<i32> {
    page_world
        .or_else(|| home_world.filter(|home| scope_worlds.contains(home)))
        .filter(|world| !excluded.contains(world))
}

/// Every recipe whose result is `item_id`, in id order.
pub(crate) fn output_recipes(item_id: i32) -> Vec<&'static Recipe> {
    let mut recipes: Vec<&'static Recipe> = tracked_data()
        .recipes
        .values()
        .filter(|recipe| recipe.item_result == item_id)
        .collect();
    recipes.sort_by_key(|recipe| recipe.key_id.0);
    recipes
}

/// The vendor anchor `real_price` uses against laundered sales — the same
/// value `RealPriceSummary` passes, so both show the same real price.
fn laundering_vendor_price(item_id: i32) -> Option<i32> {
    tracked_data()
        .items
        .get(&ItemId(item_id))
        .map(|item| item.price_mid as i32)
        .filter(|price| *price > 0)
}

fn sale_samples(sales: &[SaleHistory]) -> Vec<SaleSample> {
    sales
        .iter()
        .map(|sale| SaleSample {
            world_id: sale.world_id,
            price_per_item: sale.price_per_item,
            quantity: sale.quantity,
            hq: sale.hq,
            sold_at: sale.sold_date.and_utc().timestamp(),
        })
        .collect()
}

fn quality_chip(hq: bool) -> AnyView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    view! {
        <span class="rounded border border-[color:var(--color-outline)] px-1 text-[10px] font-bold leading-4 text-[color:var(--color-text-muted)]">
            {if hq { t!(i18n, hq).into_any() } else { t!(i18n, nq).into_any() }}
        </span>
    }
    .into_any()
}

fn one_decimal(value: f64) -> String {
    format!("{value:.1}")
}

fn whole_percent(value: f64) -> String {
    format!("{value:.0}")
}

#[component]
pub(crate) fn ItemVerdicts(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    #[prop(into)] excluded_worlds: Signal<HashSet<i32>>,
    world: Memo<String>,
    item_id: Memo<i32>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    view! {
        <section
            class="@container mt-4"
            aria-label=move || t_string!(i18n, item_verdict_region_label).to_string()
        >
            <div class="grid grid-cols-1 gap-3 @min-[40rem]:grid-cols-2">
                <SellVerdictCard listing_resource filtered_listings excluded_worlds world item_id />
            </div>
        </section>
    }
}

#[component]
fn SellVerdictCard(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    filtered_listings: Signal<ListingRows>,
    excluded_worlds: Signal<HashSet<i32>>,
    world: Memo<String>,
    item_id: Memo<i32>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let world_data = use_context::<LocalWorldData>().and_then(|data| data.0.ok());
    // The item page always provides `Cookies`; the guard keeps the card
    // buildable in a bare owner (see `item_verdicts_builds_without_contexts`).
    let home_world = use_context::<Cookies>().map(|_| use_home_world().0);
    // `now`-dependent text (sale rate, days of stock, ETA) waits for
    // hydration so the server and the first client render agree.
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| hydrated.set(true));

    view! {
        <Transition fallback=move || view! { <div class=CARD_CLASS><SingleLineSkeleton /></div> }>
            {move || {
                let show_rate = hydrated.get();
                let world_data = world_data.clone();
                listing_resource.with(|data_ref| {
                    let Some(Ok(data)) = data_ref.as_ref() else {
                        return ().into_any();
                    };
                    let scope_name = Url::unescape(&get_or_default(&world));
                    let scope = world_data
                        .as_ref()
                        .and_then(|helper| helper.lookup_world_by_name(&scope_name));
                    let page_world = scope.as_ref().and_then(|s| s.as_world().map(|w| w.id));
                    let scope_worlds: Vec<i32> = scope
                        .as_ref()
                        .map(|s| s.all_worlds().map(|w| w.id).collect())
                        .unwrap_or_default();
                    let home = home_world.and_then(|signal| {
                        with_or(&signal, None, |w| w.as_ref().map(|w| w.id))
                    });
                    let excluded = get_or_default(&excluded_worlds);
                    let Some(world_id) = sell_world(page_world, &scope_worlds, &excluded, home)
                    else {
                        return view! {
                            <div class=CARD_CLASS data-testid="sell-verdict">
                                <p class=MUTED>{t!(i18n, item_verdict_sell_pick_world)}</p>
                            </div>
                        }
                        .into_any();
                    };
                    let world_name = world_data
                        .as_ref()
                        .and_then(|helper| helper.lookup_selector(AnySelector::World(world_id)))
                        .map(|w| w.get_name().to_string())
                        .unwrap_or_default();
                    let listings: Vec<ListingSample> = get_or_default(&filtered_listings)
                        .iter()
                        .map(|(listing, _)| ListingSample {
                            world_id: listing.world_id,
                            price_per_unit: listing.price_per_unit,
                            quantity: listing.quantity,
                            hq: listing.hq,
                        })
                        .collect();
                    let verdict = sell_verdict(
                        &listings,
                        &sale_samples(&data.sales),
                        world_id,
                        laundering_vendor_price(get_or_default(&item_id)),
                        chrono::Utc::now().timestamp(),
                    );
                    sell_card_body(verdict, world_name, scope_name, show_rate)
                })
            }}
        </Transition>
    }
}

fn sell_card_body(
    verdict: SellVerdict,
    world_name: String,
    scope_name: String,
    show_rate: bool,
) -> AnyView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let heading = t_string!(i18n, item_verdict_sell_heading, world = world_name.as_str()).to_string();
    let board = match verdict.board {
        Some(board) => board_lines(board, verdict.rate, show_rate),
        None => {
            let quality = if verdict.hq {
                t_string!(i18n, hq).to_string()
            } else {
                t_string!(i18n, nq).to_string()
            };
            view! {
                <p class=MUTED>
                    {t_string!(i18n, item_verdict_no_listings, quality = quality.as_str(), world = world_name.as_str()).to_string()}
                </p>
            }
            .into_any()
        }
    };
    let real = match verdict.real_price {
        Some(real) => view! {
            <div class="flex flex-wrap items-center gap-x-1.5">
                <span class="text-blue-300">{t!(i18n, real_price)}</span>
                <Gil amount=real />
                {verdict.real_price_scope_wide.then(|| view! {
                    <span class=MUTED>
                        {t_string!(i18n, item_verdict_real_price_across, scope = scope_name.as_str()).to_string()}
                    </span>
                })}
            </div>
        }
        .into_any(),
        None => view! { <p class=MUTED>{t!(i18n, item_verdict_no_recent_sales)}</p> }.into_any(),
    };
    view! {
        <div class=CARD_CLASS data-testid="sell-verdict">
            <div class="flex items-center justify-between gap-2">
                <h2 class="text-base font-bold text-brand-200">{heading}</h2>
                {quality_chip(verdict.hq)}
            </div>
            {board}
            {real}
        </div>
    }
    .into_any()
}

fn board_lines(board: BoardVerdict, rate: SaleRate, show_rate: bool) -> AnyView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let wall = board.wall;
    let patient = board.patient.map(|patient| {
        let eta = board
            .patient_eta_days
            .filter(|_| show_rate)
            .map(|days| t_string!(i18n, item_verdict_eta_days, days = one_decimal(days).as_str()).to_string());
        view! {
            <div class="flex items-baseline justify-between gap-2">
                <span class=MUTED>{t!(i18n, item_verdict_patient)}</span>
                <span class="flex flex-wrap items-baseline justify-end gap-x-1.5">
                    <span class="font-bold"><Gil amount=patient /></span>
                    <span class=MUTED>
                        {t_string!(i18n, item_verdict_patient_hint, units = wall.units.to_string().as_str()).to_string()}
                    </span>
                    {eta.map(|eta| view! { <span class=MUTED>"· "{eta}</span> })}
                </span>
            </div>
        }
    });
    let gap = board.gap.map(|gap| {
        view! {
            <div class=format!("flex flex-wrap items-center gap-x-1.5 {MUTED}")>
                <span>{t_string!(i18n, item_verdict_gap, percent = whole_percent(gap.percent).as_str()).to_string()}</span>
                <span>"→"</span>
                <Gil amount=gap.price />
            </div>
        }
    });
    let stock = show_rate.then(|| match (rate, board.days_of_stock) {
        (SaleRate::UnitsPerDay(_), Some(days)) => view! {
            <p class=MUTED>{t_string!(i18n, item_verdict_days_of_stock, days = one_decimal(days).as_str()).to_string()}</p>
        }
        .into_any(),
        _ => view! { <p class=MUTED>{t!(i18n, item_verdict_too_few_sales)}</p> }.into_any(),
    });
    let warning = board.warning.map(|warning| {
        let text = match warning {
            FloorWarning::UnderRealPrice { percent } => t_string!(
                i18n,
                item_verdict_floor_under_real,
                percent = whole_percent(percent).as_str()
            )
            .to_string(),
            FloorWarning::AboveRecentSales => t_string!(i18n, item_verdict_floor_above_real).to_string(),
        };
        view! {
            <span class="self-start rounded-full border border-amber-300/40 bg-amber-500/10 px-2 py-0.5 text-xs text-amber-200">
                {text}
            </span>
        }
    });
    view! {
        <div class="flex items-baseline justify-between gap-2">
            <span class=MUTED>{t!(i18n, item_verdict_fast)}</span>
            <span class="flex flex-wrap items-baseline justify-end gap-x-1.5">
                <span class="font-bold"><Gil amount=board.fast /></span>
                <span class=MUTED>{t!(i18n, item_verdict_fast_hint)}</span>
            </span>
        </div>
        {patient}
        <div class="my-1 border-t border-[color:var(--color-outline)]"></div>
        <div class=format!("flex flex-wrap items-center gap-x-1.5 {MUTED}")>
            <span>{t!(i18n, item_verdict_wall)}":"</span>
            <span>
                {t_string!(
                    i18n,
                    item_verdict_wall_counts,
                    listings = wall.listings.to_string().as_str(),
                    units = wall.units.to_string().as_str()
                ).to_string()}
            </span>
            <Gil amount=wall.low />
            {(wall.high != wall.low).then(|| view! { <span>"–"</span><Gil amount=wall.high /> })}
        </div>
        {gap}
        {stock}
        {warning}
    }
    .into_any()
}
```

Notes for the implementer:
- If `t_string!` rejects `&str` interpolation values, pass owned `String`s instead (`world = world_name.clone()`); `item_view.rs` passes `name = item_name().to_string()` successfully.
- If `quality_chip`'s `{if … }` fails to type-check, compare with `RealPriceSummary`'s identical expression (`item_view.rs:688`).
- The `":"`, `"·"`, `"→"`, `"–"` literals are punctuation, as in `DecisionHeader` (`item_view.rs:613`, `617`).

- [ ] **Step 3: Mount it and clean up the lens comments**

In `item_view.rs`'s `ListingsContent` view, change:

```rust
            <div id="overview" class="scroll-mt-16">
                <crate::routes::item_compare::FlipRouteCard item_id world listing_resource />
                <DecisionHeader listing_resource filtered_listings world item_id />
            </div>
```

to:

```rust
            <div id="overview" class="scroll-mt-16">
                <crate::routes::item_compare::FlipRouteCard item_id world listing_resource />
                <DecisionHeader listing_resource filtered_listings world item_id />
                <crate::routes::item_view_verdicts::ItemVerdicts
                    listing_resource
                    filtered_listings
                    excluded_worlds
                    world
                    item_id
                />
            </div>
```

(`excluded_worlds` is already a prop of `ListingsContent`.)

In `item_view_scope.rs`, change the doc lines:

```rust
/// Switching worlds must not discard the reader's filters: `?exclude-worlds=`,
/// `?compare-buy-from=`, and `?lens=` once the lens work lands.
```

to:

```rust
/// Switching worlds must not discard the reader's filters: `?exclude-worlds=`
/// and `?compare-buy-from=`.
```

In `item_view_sections.rs`, change the module doc:

```rust
//! Jump-nav destinations for the item view.
//!
//! The order here is the page's DOM order. Later lens work reorders the
//! rendered sections with CSS `order` while leaving this DOM order — and
//! therefore this list — untouched.
```

to:

```rust
//! Jump-nav destinations for the item view.
//!
//! The order here is the page's DOM order.
```

- [ ] **Step 4: Run the tests**

Run: `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros-app --lib item_view_verdicts`
Expected: 6 tests PASS.

Also run the existing item-view tests to confirm nothing regressed:
`CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros-app --lib item_view`
Expected: PASS.

- [ ] **Step 5: Check the hydrate build and lint, commit**

```bash
cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
git add ultros-frontend/ultros-app/src/routes/item_view_verdicts.rs ultros-frontend/ultros-app/src/routes/mod.rs ultros-frontend/ultros-app/src/routes/item_view.rs ultros-frontend/ultros-app/src/routes/item_view_scope.rs ultros-frontend/ultros-app/src/routes/item_view_sections.rs
git commit -m "feat(item): sell verdict card in the item overview

Fast and patient list prices, the undercut wall and gap above it, and
days of stock for the page world (or the home world on a DC/region
page). Drops the stale ?lens= comments.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Expected: both commands succeed, `REAL_EXIT=0`. If the wasm target is not installed, `rustup target add wasm32-unknown-unknown` first.

---

### Task 5: Craft verdict card

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_view_verdicts.rs`

**Interfaces:**
- Consumes: Task 2's `buy_price`, `BuyPrice`, `BuySource`, `craft_unit_cost`, `craft_verdict`, `CraftVerdict`, `headline_hq`; Task 4's `CARD_CLASS`, `MUTED`, `quality_chip`, `sale_samples`, `laundering_vendor_price`, `output_recipes`, `whole_percent`; existing `compute_cost<P: PriceLookup + ?Sized>(recipe: &Recipe, prices: &P, recipes_by_output: &HashMap<ItemId, Vec<&'static Recipe>>, opts: &CraftingCostOptions<'_>, is_shard: &dyn Fn(ItemId) -> bool) -> CostBreakdown` (fields used: `cost: i32`, `unpriced_market_lines: u16`), `CraftingCostOptions { require_hq, max_subcraft_depth, shards, on_hand, vendor_prices }`, `ShardsMode::{ExcludeShards, IncludeMarket}`, `OnHand`, `EmptyOnHand`, `LocalOnHand::from_map`, `OnHandMap(pub RwSignal<HashMap<i32, i32>>)`, `vendor_price_map()`, `is_shard_item(ItemId) -> bool`, `get_vendor_price(i32) -> Option<u32>`, `CheapestPrices::demand() -> LocalResource<Result<CheapestListingsMap, AppError>>`, `CheapestListingsMap { map: HashMap<CheapestListingMapKey, CheapestListingData { price, .. }> }`, `CraftOptions { exclude_shards, use_on_hand, .. }` (implements `Default`) under cookie `craft_options::COOKIE_NAME`, `get_price_zone() -> (Signal<Option<OwnedResult>>, _)`, `Section::Sources.href()`.
- Produces: `pub(crate) fn subcraft_recipes(roots: &[&'static Recipe], depth: u8) -> HashMap<ItemId, Vec<&'static Recipe>>`, `const SUBCRAFT_DEPTH: u8 = 2`.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests`:

```rust
    /// A recipe with an ingredient that is itself craftable.
    fn recipe_with_craftable_ingredient() -> (&'static Recipe, ItemId) {
        let data = tracked_data();
        data.recipes
            .values()
            .find_map(|recipe| {
                IngredientsIter::new(recipe)
                    .map(|(ingredient, _)| ingredient)
                    .find(|ingredient| {
                        data.recipes.values().any(|sub| ItemId(sub.item_result) == *ingredient)
                    })
                    .map(|ingredient| (recipe, ingredient))
            })
            .expect("game data has a recipe with a craftable ingredient")
    }

    #[test]
    fn subcraft_recipes_indexes_craftable_ingredients() {
        let (recipe, ingredient) = recipe_with_craftable_ingredient();
        let index = subcraft_recipes(&[recipe], 1);
        let subs = index.get(&ingredient).expect("ingredient is indexed");
        assert!(subs.iter().all(|sub| ItemId(sub.item_result) == ingredient));
        assert!(index.len() < tracked_data().recipes.len());
    }

    #[test]
    fn subcraft_recipes_at_depth_zero_is_empty() {
        let (recipe, _) = recipe_with_craftable_ingredient();
        assert!(subcraft_recipes(&[recipe], 0).is_empty());
    }
```

Run: `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros-app --lib item_view_verdicts`
Expected: compile error `cannot find function subcraft_recipes`.

- [ ] **Step 2: Implement `subcraft_recipes` and `CraftVerdictCard`**

Extend the imports at the top of `item_view_verdicts.rs`:

```rust
use std::collections::{HashMap, HashSet};

use ultros_api_types::cheapest_listings::CheapestListingMapKey;
use ultros_calc::verdict::{
    BoardVerdict, BuySource, CraftVerdict, FloorWarning, ListingSample, SaleRate, SaleSample,
    SellVerdict, buy_price, craft_unit_cost, craft_verdict, headline_hq, sell_verdict,
};

use crate::components::crafting_cost::{
    CraftingCostOptions, EmptyOnHand, IngredientsIter, OnHand, ShardsMode, compute_cost,
    vendor_price_map,
};
use crate::components::on_hand_input::{LocalOnHand, OnHandMap};
use crate::components::related_items::{get_vendor_price, is_shard_item};
use crate::global_state::cheapest_prices::CheapestPrices;
use crate::global_state::craft_options::{self, CraftOptions};
use crate::global_state::home_world::{get_price_zone, use_home_world};
use crate::routes::item_view_sections::Section;
```

(Replace the earlier `use std::collections::HashSet;`, `use ultros_calc::verdict::{…}` and `use crate::global_state::home_world::use_home_world;` lines with these.)

Add below `output_recipes`:

```rust
/// How many levels of intermediate crafts the craft verdict considers.
const SUBCRAFT_DEPTH: u8 = 2;

/// Recipes for the items up to `depth` levels below `roots`, keyed by the
/// item they make — the `recipes_by_output` map `compute_cost` walks for
/// sub-crafts, limited to what these recipes can reach.
pub(crate) fn subcraft_recipes(
    roots: &[&'static Recipe],
    depth: u8,
) -> HashMap<ItemId, Vec<&'static Recipe>> {
    let all = &tracked_data().recipes;
    let mut index: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
    let mut frontier: HashSet<ItemId> = roots
        .iter()
        .flat_map(|recipe| IngredientsIter::new(recipe).map(|(item, _)| item))
        .collect();
    for _ in 0..depth {
        if frontier.is_empty() {
            break;
        }
        let found: Vec<&'static Recipe> = all
            .values()
            .filter(|recipe| {
                let output = ItemId(recipe.item_result);
                frontier.contains(&output) && !index.contains_key(&output)
            })
            .collect();
        frontier = found
            .iter()
            .flat_map(|recipe| IngredientsIter::new(recipe).map(|(item, _)| item))
            .collect();
        for recipe in found {
            index
                .entry(ItemId(recipe.item_result))
                .or_default()
                .push(recipe);
        }
    }
    index
}
```

Change `ItemVerdicts` to add the craft card when the item has a recipe:

```rust
#[component]
pub(crate) fn ItemVerdicts(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    #[prop(into)] excluded_worlds: Signal<HashSet<i32>>,
    world: Memo<String>,
    item_id: Memo<i32>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let craftable = Memo::new(move |_| !output_recipes(get_or_default(&item_id)).is_empty());
    view! {
        <section
            class="@container mt-4"
            aria-label=move || t_string!(i18n, item_verdict_region_label).to_string()
        >
            <div class="grid grid-cols-1 gap-3 @min-[40rem]:grid-cols-2">
                <SellVerdictCard listing_resource filtered_listings excluded_worlds world item_id />
                <Show when=move || get_or_default(&craftable)>
                    <CraftVerdictCard listing_resource item_id />
                </Show>
            </div>
        </section>
    }
}
```

Add the craft card:

```rust
#[component]
fn CraftVerdictCard(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    item_id: Memo<i32>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let cheapest = use_context::<CheapestPrices>().map(|prices| prices.demand());
    let cookies = use_context::<Cookies>();
    let options = cookies
        .as_ref()
        .map(|cookies| cookies.use_cookie_typed::<_, CraftOptions>(craft_options::COOKIE_NAME).0);
    let price_zone = cookies.map(|_| get_price_zone().0);
    let on_hand_map = use_context::<OnHandMap>();
    // `CheapestPrices` is a client-only resource: render the skeleton on the
    // server and during hydration, then the verdict (the #740 idiom).
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| hydrated.set(true));

    view! {
        <Transition fallback=move || view! { <div class=CARD_CLASS><SingleLineSkeleton /></div> }>
            {move || {
                let item = get_or_default(&item_id);
                let hq = listing_resource.with(|data_ref| {
                    data_ref
                        .as_ref()
                        .and_then(|result| result.as_ref().ok())
                        .map(|data| headline_hq(&sale_samples(&data.sales), laundering_vendor_price(item)))
                });
                let Some(hq) = hq else {
                    return ().into_any();
                };
                if !hydrated.get() {
                    return view! { <div class=CARD_CLASS><SingleLineSkeleton /></div> }.into_any();
                }
                let Some(cheapest) = cheapest else {
                    return ().into_any();
                };
                cheapest
                    .with(|prices| {
                        let prices = prices.as_ref()?.as_ref().ok()?;
                        let opts = options.and_then(|cookie| cookie.get()).unwrap_or_default();
                        let shards = if opts.exclude_shards {
                            ShardsMode::ExcludeShards
                        } else {
                            ShardsMode::IncludeMarket
                        };
                        let recipes = output_recipes(item);
                        let index = subcraft_recipes(&recipes, SUBCRAFT_DEPTH);
                        let (craft_unit, unpriced) = recipes
                            .iter()
                            .map(|recipe| {
                                // A fresh on-hand snapshot per run: compute_cost consumes it.
                                let local = LocalOnHand::from_map(
                                    on_hand_map.map(|map| map.0.get()).unwrap_or_default(),
                                );
                                let empty = EmptyOnHand;
                                let on_hand: &dyn OnHand =
                                    if opts.use_on_hand { &local } else { &empty };
                                let cost_options = CraftingCostOptions {
                                    require_hq: hq,
                                    max_subcraft_depth: SUBCRAFT_DEPTH,
                                    shards,
                                    on_hand,
                                    vendor_prices: Some(vendor_price_map()),
                                };
                                let breakdown =
                                    compute_cost(recipe, prices, &index, &cost_options, &is_shard_item);
                                (
                                    craft_unit_cost(breakdown.cost, recipe.amount_result),
                                    breakdown.unpriced_market_lines,
                                )
                            })
                            // Fully priced runs first, then the cheapest.
                            .min_by_key(|&(unit, unpriced)| (unpriced > 0, unit))?;
                        let market = |hq| {
                            prices
                                .map
                                .get(&CheapestListingMapKey { item_id: item, hq })
                                .map(|listing| listing.price)
                        };
                        let buy = buy_price(
                            hq,
                            market(true),
                            market(false),
                            get_vendor_price(item).map(|price| price as i32),
                        );
                        let zone = price_zone
                            .and_then(|zone| zone.get())
                            .map(|zone| zone.get_name().to_string())
                            .unwrap_or_else(|| "North-America".to_string());
                        let verdict = craft_verdict(craft_unit, buy.map(|buy| buy.price), unpriced);
                        Some(craft_card_body(hq, craft_unit, buy, verdict, zone, opts.exclude_shards))
                    })
                    .unwrap_or_else(|| ().into_any())
            }}
        </Transition>
    }
}

fn craft_card_body(
    hq: bool,
    craft_unit: i32,
    buy: Option<ultros_calc::verdict::BuyPrice>,
    verdict: CraftVerdict,
    zone: String,
    crystals_excluded: bool,
) -> AnyView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let buy_label = match buy.map(|buy| buy.source) {
        Some(BuySource::Vendor) => t_string!(i18n, item_verdict_buy_vendor).to_string(),
        _ => t_string!(i18n, item_verdict_buy_zone, zone = zone.as_str()).to_string(),
    };
    let nq_fallback = matches!(
        buy.map(|buy| buy.source),
        Some(BuySource::Market { nq_fallback: true })
    );
    let chip = |class: &'static str, label: AnyView, gil: Option<(i32, f64)>| {
        view! {
            <span class=format!("inline-flex items-center gap-1 self-start rounded-full border px-2 py-0.5 text-xs font-semibold {class}")>
                {label}
                {gil.map(|(gil, percent)| view! {
                    <Gil amount=gil />
                    <span>"("{whole_percent(percent)}"%)"</span>
                })}
            </span>
        }
        .into_any()
    };
    let verdict_view = match verdict {
        CraftVerdict::CraftSaves { gil, percent } => chip(
            "border-emerald-400/40 bg-emerald-500/10 text-emerald-200",
            t!(i18n, item_verdict_craft_saves).into_any(),
            Some((gil, percent)),
        ),
        CraftVerdict::BuyCheaper { gil, percent } => chip(
            "border-red-400/40 bg-red-500/10 text-red-200",
            t!(i18n, item_verdict_buy_cheaper).into_any(),
            Some((gil, percent)),
        ),
        CraftVerdict::AboutEven => chip(
            "border-[color:var(--color-outline)] text-[color:var(--color-text-muted)]",
            t!(i18n, item_verdict_about_even).into_any(),
            None,
        ),
        CraftVerdict::Incomplete => {
            view! { <p class=MUTED>{t!(i18n, item_verdict_incomplete)}</p> }.into_any()
        }
    };
    view! {
        <div class=CARD_CLASS data-testid="craft-verdict">
            <div class="flex items-center justify-between gap-2">
                <h2 class="text-base font-bold text-brand-200">{t!(i18n, item_verdict_craft_heading)}</h2>
                {quality_chip(hq)}
            </div>
            <div class="flex items-baseline justify-between gap-2">
                <span class=MUTED>{t!(i18n, item_verdict_craft_unit)}</span>
                <span class="font-bold"><Gil amount=craft_unit /></span>
            </div>
            <div class="flex items-baseline justify-between gap-2">
                <span class=format!("flex items-center gap-1 {MUTED}")>
                    {buy_label}
                    {nq_fallback.then(|| quality_chip(false))}
                </span>
                <span class="font-bold">
                    {match buy {
                        Some(buy) => view! { <Gil amount=buy.price /> }.into_any(),
                        None => t!(i18n, no_data).into_any(),
                    }}
                </span>
            </div>
            {verdict_view}
            <p class=format!("text-xs {MUTED}")>
                {t!(i18n, item_verdict_incl_subcrafts)}
                {crystals_excluded.then(|| view! { " · "{t!(i18n, item_verdict_crystals_excluded)} })}
            </p>
            <a class="self-start text-xs underline text-brand-300 hover:text-brand-200" href=Section::Sources.href()>
                {t!(i18n, item_verdict_recipe_link)}" ↓"
            </a>
        </div>
    }
    .into_any()
}
```

Notes for the implementer:
- `cheapest.with(...)` returns `Option<AnyView>` here; the closure uses `?` the same way `RecipePriceEstimate` does (`related_items.rs:187-249`).
- `compute_cost(recipe, …)`: `recipe` is `&&'static Recipe` from the iterator; pass `recipe` (auto-deref) or `*recipe` if the compiler asks.
- `"North-America"` is the same data identifier the `CheapestPrices` resource defaults to (`cheapest_prices.rs`); it is a world-group name, not UI copy.

- [ ] **Step 3: Run the tests**

Run: `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros-app --lib item_view_verdicts`
Expected: 8 tests PASS.

- [ ] **Step 4: Check the hydrate build and lint, commit**

```bash
cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
git add ultros-frontend/ultros-app/src/routes/item_view_verdicts.rs
git commit -m "feat(item): craft-or-buy verdict card

Cheapest recipe's per-unit cost (sub-crafts to depth 2, honoring the
craft-options cookie) against the price zone's cheapest listing, or the
NPC vendor when that is cheaper for NQ.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Browser verification

**Files:** none (verification only; fix-ups go in a new commit).

- [ ] **Step 1: Serve locally**

Follow `AGENTS.md` / the local-serve notes: `cargo leptos watch` (or the project's documented serve command) with a relative `LEPTOS_SITE_ROOT`, a free `METRICS_PORT`, and port 8080 free. Wait for "listening on".

- [ ] **Step 2: Check each case in the browser and the console**

| URL | Expect |
|---|---|
| `/item/Gilgamesh/5057` (Iron Ingot: craftable, vendor-sold) | Sell card for Gilgamesh with fast/patient/wall; craft card with "Buy · vendor" or zone label; no console hydration warnings |
| `/item/Aether/5057` with home world Gilgamesh set | Sell card says "Sell on Gilgamesh" |
| `/item/Aether/5057?exclude-worlds=<Gilgamesh id>` with home world Gilgamesh | Sell card shows "Pick a world to see where to list" |
| `/item/Aether/5057` with no home world cookie | "Pick a world…" hint |
| A multi-yield recipe item (e.g. a cooking recipe yielding 3) | Craft / unit is roughly the recipe cards' Est. cost ÷ yield |
| A non-craftable item (e.g. `/item/Gilgamesh/5111`, a crystal) | Only the sell card, half width on a wide window |
| Any item, locale switched to 日本語 | All card text is Japanese |

Resize to 375 px wide: the cards stack, no horizontal scroll.

- [ ] **Step 3: Commit any fixes**

Only if Step 2 required changes:

```bash
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
git add -A ultros-frontend/
git commit -m "fix(item): verdict card fixes from browser check

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
