# Analyzer Kit — Phase A: Formula Core, Zero-Copy Pricing, Memo Split — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the kit's formula model, the zero-copy price views, and the recipe analyzer's compute/sort memo split with byte-identical output, as one PR against `main` with no user-visible change.

**Architecture:** A new `analyzer_kit` module (`formula.rs`, `signals.rs`, `needed.rs`) holds pure, DOM-free types the recipe analyzer consumes immediately: `PriceSignal` (the old `CostBasis`/`RevenueMetric` unified), `ProfitFormula` with per-tool policies, `profit_line` as the one drop rule, a `PriceLookup` trait with a lazy `SignalView` that replaces the three full-map clones per basis change, and `needed_bodies` as the single fetch gate. The recipe analyzer's monolithic `computed_data` memo is extracted into two pure functions, `price_rows` and `filter_and_sort`, so a header click never re-prices 7k recipes. Spec: `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md` (Phase A) and `2026-09-01-recipe-analyzer-profit-formula-columns-design.md` (Phase 1a).

**Tech Stack:** Rust 2024 edition, Leptos 0.8 (`ArcResource`, `Memo`, `filter_query_signal`), `ultros-api-types`, `xiv_gen` game data via `xiv_gen_db::data()`.

## Global Constraints

- Every user-facing string goes through `leptos-i18n` in all 7 locales; this phase adds none.
- `./check_ci.sh` (fmt + `clippy --all-targets -- -D warnings`) must pass before the PR; CI runs no tests, so run `cargo test -p ultros-app --lib` locally.
- Every module in `ultros-app` is `pub(crate)`: an enum variant, struct field or function with no non-test consumer fails clippy's `dead_code`. Introduce only what this phase consumes (the ledger at the end of each task says what).
- Zero user-visible change: the page's numbers, row set, fetches and markup are identical to `main`. The one deliberate ordering change is the deterministic key-id tiebreak inside sort ties (Task 6). The characterization test in Task 6 is the gate.
- URL keys and tokens are a pinned contract: `cost-basis`, `revenue`, `buy-scope`, tokens `listing-min|sale-median|sale-min|sale-avg`, `world|datacenter|region`, and the 11 `?sort=` tokens. The existing tests `url_values_round_trip`, `defaults`, `world_min_token_no_longer_parses`, `filter_registry_keys_are_a_stable_url_contract`, `sort_mode_round_trips_through_the_url`, `legacy_*` must keep passing unchanged.
- No changelog entry (purely internal work, `changelog.rs:30-32`).
- Commit after every task; the PR targets `main` ("Part of #1233", never "closes").
- Do not `#[allow(dead_code)]` new code.

---

## File map

| File | Responsibility |
|---|---|
| `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs` (new) | module root |
| `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs` (new) | `SaleStat`, `PriceSignal` (+ `CostBasis`/`RevenueMetric` aliases), `BuyScope`, `Term`, estimators, `TaxPolicy`/`TaxMath`/`RoiMath`/`DropRule`, `ProfitFormula`, `ProfitLine`, `profit_line`, `sale_tax_for`, `per_unit_cost`, `effective` |
| `ultros-frontend/ultros-app/src/analyzer_kit/signals.rs` (new) | `PriceLookup`, `StatsIndex`, `stats_index`, `SignalView` |
| `ultros-frontend/ultros-app/src/analyzer_kit/needed.rs` (new) | `BodyRole`, `needed_bodies` |
| `ultros-frontend/ultros-app/src/price_basis.rs` (modify) | becomes re-exports + the two legacy helpers as test-only oracles |
| `ultros-frontend/ultros-app/src/components/crafting_cost.rs` (modify) | `compute_cost`, `compute_cost_inner`, `compute_ingredient_cost` generic over `P: PriceLookup + ?Sized` |
| `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (modify) | `price_rows` + `filter_and_sort` pure fns, `SellHistory` resource, `?sort=`/`?dir=` hoisted to the page, stale comments fixed |
| `ultros-frontend/ultros-app/src/lib.rs` (modify) | `pub(crate) mod analyzer_kit;` |

---

### Task 1: Kit skeleton and `PriceSignal` unification

**Files:**
- Create: `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs`
- Create: `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs`
- Modify: `ultros-frontend/ultros-app/src/price_basis.rs` (whole file)
- Modify: `ultros-frontend/ultros-app/src/lib.rs:2-14`

**Interfaces:**
- Consumes: nothing.
- Produces: `analyzer_kit::formula::{SaleStat, PriceSignal, CostBasis, RevenueMetric, BuyScope}`; `price_basis` re-exports the same names so `recipe_analyzer.rs` compiles unchanged.

- [ ] **Step 1: Write the failing test** in `formula.rs` (the file does not exist yet, so create it with only the test module first)

```rust
// ultros-frontend/ultros-app/src/analyzer_kit/formula.rs
//! The profit ledger: which signal feeds each side of
//! `profit = revenue − tax − cost`, and the per-tool policies that make
//! every analyzer's numbers reproducible from one function.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_signal_tokens_are_unchanged() {
        // These four tokens are the `cost-basis` / `revenue` URL contract.
        assert_eq!(PriceSignal::ListingMin.to_string(), "listing-min");
        assert_eq!(PriceSignal::SaleMedian.to_string(), "sale-median");
        assert_eq!(PriceSignal::SaleMin.to_string(), "sale-min");
        assert_eq!(PriceSignal::SaleAvg.to_string(), "sale-avg");
        for s in ["listing-min", "sale-median", "sale-min", "sale-avg"] {
            assert_eq!(s.parse::<PriceSignal>().unwrap().to_string(), s);
        }
        assert!("world-min".parse::<PriceSignal>().is_err());
        assert_eq!(PriceSignal::default(), PriceSignal::ListingMin);
        assert_eq!(PriceSignal::ListingMin.sale_stat(), None);
        assert_eq!(PriceSignal::SaleMedian.sale_stat(), Some(SaleStat::Median));
    }

    #[test]
    fn aliases_are_the_same_type() {
        let cost: CostBasis = PriceSignal::SaleMin;
        let revenue: RevenueMetric = cost;
        assert_eq!(revenue, PriceSignal::SaleMin);
    }
}
```

- [ ] **Step 2: Create `mod.rs` and register the module, then run the test to verify it fails**

```rust
// ultros-frontend/ultros-app/src/analyzer_kit/mod.rs
//! Shared building blocks for the profit analyzers: the formula ledger,
//! zero-copy price views and the fetch gate. See
//! docs/superpowers/specs/2026-09-01-analyzer-kit-design.md.
pub mod formula;
```

Add to `lib.rs` after `pub(crate) mod analysis;`:

```rust
pub(crate) mod analyzer_kit;
```

Run: `cargo test -p ultros-app --lib analyzer_kit::formula`
Expected: FAIL to compile with `cannot find type PriceSignal`.

- [ ] **Step 3: Move the enums into `formula.rs`**

Replace the top of `formula.rs` (above the test module) with:

```rust
use std::fmt::{self, Display};
use std::str::FromStr;

/// Which sale-history statistic a signal reads from an
/// [`ultros_api_types::sale_stats::ItemSaleStats`] row.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SaleStat {
    Min,
    Median,
    Avg,
}

/// A price signal: the cheapest current listing, or a trailing-window sale
/// statistic. The same four signals price ingredients (over the buy scope)
/// and revenue (on the sell world); the URL tokens are the `cost-basis` /
/// `revenue` bookmark contract and never change.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum PriceSignal {
    #[default]
    ListingMin,
    SaleMin,
    SaleMedian,
    SaleAvg,
}

/// Ingredient-side name for [`PriceSignal`], kept so the route reads as
/// it did when the two sides were separate enums.
pub type CostBasis = PriceSignal;
/// Revenue-side name for [`PriceSignal`].
pub type RevenueMetric = PriceSignal;

impl PriceSignal {
    /// The sale statistic this signal reads, or `None` for the listing.
    pub fn sale_stat(self) -> Option<SaleStat> {
        match self {
            PriceSignal::ListingMin => None,
            PriceSignal::SaleMin => Some(SaleStat::Min),
            PriceSignal::SaleMedian => Some(SaleStat::Median),
            PriceSignal::SaleAvg => Some(SaleStat::Avg),
        }
    }
}

impl FromStr for PriceSignal {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "listing-min" => Ok(PriceSignal::ListingMin),
            "sale-median" => Ok(PriceSignal::SaleMedian),
            "sale-min" => Ok(PriceSignal::SaleMin),
            "sale-avg" => Ok(PriceSignal::SaleAvg),
            _ => Err(()),
        }
    }
}

impl Display for PriceSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PriceSignal::ListingMin => "listing-min",
            PriceSignal::SaleMedian => "sale-median",
            PriceSignal::SaleMin => "sale-min",
            PriceSignal::SaleAvg => "sale-avg",
        })
    }
}

/// Where ingredient prices are searched: the sell world only, its
/// datacenter, or the whole region. Defaults to the datacenter.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum BuyScope {
    World,
    #[default]
    Datacenter,
    Region,
}

impl FromStr for BuyScope {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "world" => Ok(BuyScope::World),
            "datacenter" => Ok(BuyScope::Datacenter),
            "region" => Ok(BuyScope::Region),
            _ => Err(()),
        }
    }
}

impl Display for BuyScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BuyScope::World => "world",
            BuyScope::Datacenter => "datacenter",
            BuyScope::Region => "region",
        })
    }
}
```

- [ ] **Step 4: Turn `price_basis.rs` into re-exports plus the two helpers**

Replace lines 1-165 of `price_basis.rs` (the module doc, the three enums and their impls) with:

```rust
//! Selectable price bases for the recipe analyzer (#1202, #1233).
//!
//! The enums moved to [`crate::analyzer_kit::formula`] (one `PriceSignal`
//! for both sides); this module re-exports them so existing call sites
//! keep their names, and keeps the two map-cloning helpers that the
//! zero-copy `SignalView` reproduces.

pub use crate::analyzer_kit::formula::{BuyScope, CostBasis, RevenueMetric, SaleStat};
use ultros_api_types::cheapest_listings::{
    CheapestListingData, CheapestListingMapKey, CheapestListingsMap,
};
use ultros_api_types::sale_stats::BulkSaleStats;
```

`PriceSignal` is deliberately not re-exported: nothing imports it through this module, and an unused `pub use` inside a `pub(crate)` module is an `unused_imports` error under `-D warnings`. Keep `override_listings`, `overlay_sale_stats` and the whole test module exactly as they are. The tests `url_values_round_trip`, `defaults` and `world_min_token_no_longer_parses` still compile against the re-exports.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ultros-app --lib analyzer_kit::formula price_basis`
Expected: PASS (2 new + 9 existing).

- [ ] **Step 6: Clippy, fmt, commit**

Run: `cargo fmt --all && cargo clippy -p ultros-app --all-targets -- -D warnings`
Expected: clean.

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit ultros-frontend/ultros-app/src/price_basis.rs ultros-frontend/ultros-app/src/lib.rs
git commit -m "refactor(analyzer-kit): unify CostBasis/RevenueMetric into PriceSignal"
```

Ledger: `SaleStat::{Min,Median,Avg}`, `PriceSignal` (4), `BuyScope` (3) — all constructed by the route today through the re-exports.

---

### Task 2: `ProfitFormula`, `profit_line` and the per-tool policies

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/formula.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:203-216` (remove `per_unit_cost`, `MARKET_TAX_PERCENT`, `net_after_tax`) and the two tests `per_unit_cost_divides_by_yield`, `net_after_tax_takes_five_percent` (2212-2231, their `///` doc comments included)

**Interfaces:**
- Consumes: `PriceSignal`, `BuyScope`, `SaleStat` from Task 1; `crate::analysis::ROI_DISPLAY_CEILING` exists but is not used here.
- Produces:
  - `pub enum Term<T: Copy> { Fixed(T), Select(T) }` with `fn value(self) -> T`
  - `pub enum RevenueEstimator { Signal(PriceSignal) }`, `pub enum CostEstimator { Craft(PriceSignal) }`
  - `pub enum TaxPolicy { MarketBoard }`, `pub enum TaxMath { IntegerFloor }`, `pub enum RoiMath { UnclampedF64 }`, `pub enum DropRule { CostAtOrAboveNet }`
  - `pub struct ProfitFormula { revenue, sell_scope, cost, tax, tax_math, roi, drop }`
  - `ProfitFormula::recipe_from_query(cost: Option<CostBasis>, revenue: Option<RevenueMetric>, scope: Option<BuyScope>) -> ProfitFormula`
  - `ProfitFormula::effective(self, buy_stats_loaded: bool, sell_stats_loaded: bool) -> ProfitFormula`
  - `ProfitFormula::cost_signal(&self) -> PriceSignal`, `revenue_signal(&self) -> PriceSignal`, `buy_scope(&self) -> BuyScope`
  - `pub struct ProfitLine { revenue, tax, net, cost, profit, roi }`
  - `pub fn profit_line(gross: i32, cost_per_unit: i32, f: &ProfitFormula) -> (ProfitLine, bool)`
  - `pub fn sale_tax_for(gross: i32, math: TaxMath) -> i32`, `pub fn net_after_tax(gross: i32, math: TaxMath) -> i32`
  - `pub fn per_unit_cost(craft_cost: i32, amount_result: i32) -> i32`

- [ ] **Step 1: Write the failing tests** (append inside `mod tests`)

```rust
    fn recipe_default() -> ProfitFormula {
        ProfitFormula::recipe_from_query(None, None, None)
    }

    #[test]
    fn per_unit_cost_divides_by_yield() {
        assert_eq!(per_unit_cost(300, 3), 100);
        assert_eq!(per_unit_cost(300, 1), 300);
        assert_eq!(per_unit_cost(300, 0), 300);
        assert_eq!(per_unit_cost(100, 3), 33);
    }

    #[test]
    fn net_after_tax_takes_five_percent_floor() {
        assert_eq!(net_after_tax(100, TaxMath::IntegerFloor), 95);
        assert_eq!(net_after_tax(1, TaxMath::IntegerFloor), 0);
        assert_eq!(net_after_tax(0, TaxMath::IntegerFloor), 0);
        assert_eq!(net_after_tax(1_999_999_999, TaxMath::IntegerFloor), 1_899_999_999);
        assert_eq!(sale_tax_for(100, TaxMath::IntegerFloor), 5);
    }

    #[test]
    fn recipe_from_query_uses_defaults() {
        let f = recipe_default();
        assert_eq!(f.cost_signal(), PriceSignal::ListingMin);
        assert_eq!(f.revenue_signal(), PriceSignal::ListingMin);
        assert_eq!(f.buy_scope(), BuyScope::Datacenter);
        assert_eq!(f.sell_scope.value(), BuyScope::World);
        let f = ProfitFormula::recipe_from_query(
            Some(PriceSignal::SaleAvg),
            Some(PriceSignal::SaleMin),
            Some(BuyScope::Region),
        );
        assert_eq!(f.cost_signal(), PriceSignal::SaleAvg);
        assert_eq!(f.revenue_signal(), PriceSignal::SaleMin);
        assert_eq!(f.buy_scope(), BuyScope::Region);
    }

    #[test]
    fn profit_line_drops_when_cost_meets_net_revenue() {
        let f = recipe_default();
        // 12,560 gross → 11,932 net; 11,300 cost → 632 profit, ROI 5%.
        let (line, dropped) = profit_line(12_560, 11_300, &f);
        assert!(!dropped);
        assert_eq!(line.net, 11_932);
        assert_eq!(line.tax, 628);
        assert_eq!(line.profit, 632);
        assert_eq!(line.roi, 5);
        // Cost equal to net is dropped (today's `>=` rule).
        let (_, dropped) = profit_line(12_560, 11_932, &f);
        assert!(dropped);
        let (_, dropped) = profit_line(12_560, 11_933, &f);
        assert!(dropped);
    }

    #[test]
    fn profit_line_roi_matches_todays_unclamped_math() {
        let f = recipe_default();
        // Terminus Putty class: 999,999 gross, 261 cost.
        let (line, _) = profit_line(999_999, 261, &f);
        assert_eq!(line.net, 949_999);
        assert_eq!(line.profit, 949_738);
        assert_eq!(line.roi, 363_884);
        // Cost 0 → ROI 0, never a division by zero.
        let (line, dropped) = profit_line(100, 0, &f);
        assert!(!dropped);
        assert_eq!(line.roi, 0);
    }

    #[test]
    fn effective_downgrades_absent_sale_signal_to_listing() {
        let f = ProfitFormula::recipe_from_query(
            Some(PriceSignal::SaleMedian),
            Some(PriceSignal::SaleAvg),
            None,
        );
        let e = f.effective(false, true);
        assert_eq!(e.cost_signal(), PriceSignal::ListingMin);
        assert_eq!(e.revenue_signal(), PriceSignal::SaleAvg);
        let e = f.effective(true, false);
        assert_eq!(e.cost_signal(), PriceSignal::SaleMedian);
        assert_eq!(e.revenue_signal(), PriceSignal::ListingMin);
        let e = f.effective(true, true);
        assert_eq!(e, f);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-app --lib analyzer_kit::formula`
Expected: FAIL to compile (`ProfitFormula` not found).

- [ ] **Step 3: Add the types** (below the `BuyScope` impls, above `mod tests`)

```rust
/// A slot in the ledger: fixed by the tool, or chosen by the user.
/// Phase C gives `Select` its URL key when the strip renders it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Term<T: Copy> {
    Fixed(T),
    Select(T),
}

impl<T: Copy> Term<T> {
    pub fn value(self) -> T {
        match self {
            Term::Fixed(v) | Term::Select(v) => v,
        }
    }
}

/// How a tool estimates what one unit sells for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RevenueEstimator {
    /// A price signal on the sell place (recipe analyzer).
    Signal(PriceSignal),
}

/// How a tool estimates what one unit costs to obtain.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CostEstimator {
    /// `compute_cost` over a price view (recipe analyzer).
    Craft(PriceSignal),
}

/// Whether the market board's cut is taken off revenue.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TaxPolicy {
    MarketBoard,
}

/// How the 5% is rounded. The recipe analyzer floors in integer math;
/// the flip finder and vendor resale truncate an f32 product. The two
/// agree for every sale price below 2,207,541 gil.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TaxMath {
    IntegerFloor,
}

/// How ROI is computed. The recipe analyzer's unclamped f64 division is
/// kept here so Phase A changes no number; Phase C adopts the clamp.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RoiMath {
    UnclampedF64,
}

/// Which rows the tool removes outright.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DropRule {
    /// `cost_per_unit >= net` — the recipe analyzer's rule.
    CostAtOrAboveNet,
}

/// The ledger: `profit = revenue @ sell place − tax − cost @ buy scope`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProfitFormula {
    pub revenue: Term<RevenueEstimator>,
    pub sell_scope: Term<BuyScope>,
    pub cost: Term<CostEstimator>,
    pub buy_scope: Term<BuyScope>,
    pub tax: Term<TaxPolicy>,
    pub tax_math: TaxMath,
    pub roi: RoiMath,
    pub drop: DropRule,
}

impl ProfitFormula {
    /// The recipe analyzer's ledger from its three URL params. Absent
    /// params are the enum defaults, exactly as the route unwraps them.
    pub fn recipe_from_query(
        cost: Option<CostBasis>,
        revenue: Option<RevenueMetric>,
        scope: Option<BuyScope>,
    ) -> Self {
        Self {
            revenue: Term::Select(RevenueEstimator::Signal(revenue.unwrap_or_default())),
            sell_scope: Term::Fixed(BuyScope::World),
            cost: Term::Select(CostEstimator::Craft(cost.unwrap_or_default())),
            buy_scope: Term::Select(scope.unwrap_or_default()),
            tax: Term::Fixed(TaxPolicy::MarketBoard),
            tax_math: TaxMath::IntegerFloor,
            roi: RoiMath::UnclampedF64,
            drop: DropRule::CostAtOrAboveNet,
        }
    }

    pub fn cost_signal(&self) -> PriceSignal {
        match self.cost.value() {
            CostEstimator::Craft(s) => s,
        }
    }

    pub fn revenue_signal(&self) -> PriceSignal {
        match self.revenue.value() {
            RevenueEstimator::Signal(s) => s,
        }
    }

    pub fn buy_scope(&self) -> BuyScope {
        self.buy_scope.value()
    }

    /// The formula the numbers actually use: a sale signal whose stats
    /// body is absent (not requested, or the fetch failed) falls back to
    /// the listing, so no label ever names a signal the table ignores.
    pub fn effective(self, buy_stats_loaded: bool, sell_stats_loaded: bool) -> Self {
        let mut f = self;
        if !buy_stats_loaded && self.cost_signal().sale_stat().is_some() {
            f.cost = match self.cost {
                Term::Fixed(_) => Term::Fixed(CostEstimator::Craft(PriceSignal::ListingMin)),
                Term::Select(_) => Term::Select(CostEstimator::Craft(PriceSignal::ListingMin)),
            };
        }
        if !sell_stats_loaded && self.revenue_signal().sale_stat().is_some() {
            f.revenue = match self.revenue {
                Term::Fixed(_) => Term::Fixed(RevenueEstimator::Signal(PriceSignal::ListingMin)),
                Term::Select(_) => Term::Select(RevenueEstimator::Signal(PriceSignal::ListingMin)),
            };
        }
        f
    }
}

/// One row's arithmetic under the selected formula.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProfitLine {
    pub revenue: i32,
    pub tax: i32,
    pub net: i32,
    pub cost: i32,
    pub profit: i32,
    pub roi: i32,
}

/// The market board's cut of every sale.
const MARKET_TAX_PERCENT: i64 = 5;

/// What the seller receives from a sale listed at `gross`.
pub fn net_after_tax(gross: i32, math: TaxMath) -> i32 {
    match math {
        TaxMath::IntegerFloor => (gross as i64 * (100 - MARKET_TAX_PERCENT) / 100) as i32,
    }
}

/// The market board's cut of a sale at `gross` — the Tax column, which
/// is independent of whether the tool's profit nets it.
pub fn sale_tax_for(gross: i32, math: TaxMath) -> i32 {
    gross - net_after_tax(gross, math)
}

/// Cost of one unit of output: one craft costs `craft_cost` and yields
/// `amount_result` units. Yields of 0 (bad sheet rows) are treated as 1.
pub fn per_unit_cost(craft_cost: i32, amount_result: i32) -> i32 {
    craft_cost / amount_result.max(1)
}

/// The one place the drop rule, tax and ROI live. Always returns the
/// line; the flag says whether the tool's [`DropRule`] removes the row.
pub fn profit_line(gross: i32, cost_per_unit: i32, f: &ProfitFormula) -> (ProfitLine, bool) {
    let tax = sale_tax_for(gross, f.tax_math);
    let net = match f.tax.value() {
        TaxPolicy::MarketBoard => gross - tax,
    };
    let profit = net - cost_per_unit;
    let roi = match f.roi {
        RoiMath::UnclampedF64 => {
            if cost_per_unit > 0 {
                (profit as f64 / cost_per_unit as f64 * 100.0) as i32
            } else {
                0
            }
        }
    };
    let dropped = match f.drop {
        DropRule::CostAtOrAboveNet => cost_per_unit >= net,
    };
    (
        ProfitLine {
            revenue: gross,
            tax,
            net,
            cost: cost_per_unit,
            profit,
            roi,
        },
        dropped,
    )
}
```

- [ ] **Step 4: Point the route at the moved helpers**

In `recipe_analyzer.rs` delete lines 203-216 (`per_unit_cost`, `MARKET_TAX_PERCENT`, `net_after_tax`) and the two tests `per_unit_cost_divides_by_yield` and `net_after_tax_takes_five_percent` at 2212-2231, doc comments included (they moved to `formula.rs`). Change the import block at the top:

```rust
use crate::analyzer_kit::formula::{TaxMath, net_after_tax, per_unit_cost};
```

and the two call sites inside `computed_data` (lines 915-917):

```rust
            let cost_per_unit = per_unit_cost(craft_cost, recipe.amount_result);

            let net_revenue = net_after_tax(market_price, TaxMath::IntegerFloor);
```

Nothing else changes yet; `profit_line` gains its route consumer in Task 6.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ultros-app --lib analyzer_kit::formula recipe_analyzer`
Expected: PASS (formula: 8; recipe_analyzer: 19, the two moved tests gone).

- [ ] **Step 6: Clippy, fmt, commit**

Run: `cargo fmt --all && cargo clippy -p ultros-app --all-targets -- -D warnings`
Expected: clean. If clippy reports `profit_line` / `ProfitFormula` / `Term` as dead code, that is expected until Task 6 wires them into `price_rows`; move on only if the warning list is exactly those items, and confirm it is empty after Task 6.

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/formula.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(analyzer-kit): ProfitFormula, profit_line and per-tool policies"
```

Ledger: `Term::{Fixed, Select}`, `RevenueEstimator::Signal`, `CostEstimator::Craft`, `TaxPolicy::MarketBoard`, `TaxMath::IntegerFloor`, `RoiMath::UnclampedF64`, `DropRule::CostAtOrAboveNet` — all constructed by `recipe_from_query`, consumed by `price_rows` in Task 6.

---

### Task 3: `PriceLookup` and the zero-copy `SignalView`

**Files:**
- Create: `ultros-frontend/ultros-app/src/analyzer_kit/signals.rs`
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs` (add `pub mod signals;`)

**Interfaces:**
- Consumes: `SaleStat` (Task 1); `CheapestListingsMap`, `CheapestListingMapKey`, `CheapestListingData`, `PriceSummary` from `ultros_api_types::cheapest_listings`; `BulkSaleStats`, `ItemSaleStats` from `ultros_api_types::sale_stats`; `price_basis::{override_listings, overlay_sale_stats}` as test oracles.
- Produces:
  - `pub trait PriceLookup { fn find_matching_listings(&self, item_id: i32) -> PriceSummary; }` implemented for `CheapestListingsMap`, `&P`, `Arc<P>`
  - `pub type StatsIndex = HashMap<(i32, bool), ItemSaleStats>` and `pub fn stats_index(stats: &BulkSaleStats) -> StatsIndex`
  - `pub struct SignalView<'a> { pub over: Option<&'a CheapestListingsMap>, pub base: &'a CheapestListingsMap, pub stats: Option<(&'a StatsIndex, SaleStat)> }` implementing `PriceLookup`
  - `pub fn stat_price(row: &ItemSaleStats, stat: SaleStat) -> i32`

- [ ] **Step 1: Write the failing tests** (create `signals.rs` with the test module first)

```rust
// ultros-frontend/ultros-app/src/analyzer_kit/signals.rs
//! Price lookups the pricing core can be generic over, and the layered
//! view that prices from a sale statistic with the listing as fallback
//! without cloning any map.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::price_basis::{overlay_sale_stats, override_listings};
    use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};

    fn listings(items: &[(i32, bool, i32, i32)]) -> CheapestListingsMap {
        CheapestListingsMap::from(CheapestListings {
            cheapest_listings: items
                .iter()
                .map(|&(item_id, hq, cheapest_price, world_id)| CheapestListingItem {
                    item_id,
                    hq,
                    cheapest_price,
                    world_id,
                })
                .collect(),
        })
    }

    fn stats(rows: &[(i32, bool, i32, i32, i32)]) -> BulkSaleStats {
        BulkSaleStats {
            stats: rows
                .iter()
                .map(|&(item_id, hq, min_price, median_price, avg_price)| ItemSaleStats {
                    item_id,
                    hq,
                    min_price,
                    median_price,
                    avg_price,
                    num_sold: 10,
                    ..Default::default()
                })
                .collect(),
        }
    }

    /// Items 1-4 cover: listed both sides, listed only in base, listed
    /// only in over, stats without any listing, and a zero-priced stat.
    fn fixture() -> (CheapestListingsMap, CheapestListingsMap, BulkSaleStats) {
        let base = listings(&[(1, false, 100, 7), (1, true, 180, 7), (2, false, 200, 7), (5, false, 50, 7)]);
        let over = listings(&[(1, false, 150, 42), (3, true, 300, 42)]);
        let stats = stats(&[
            (1, false, 90, 120, 130),
            (2, true, 210, 220, 230),
            (4, false, 10, 20, 30),
            (5, false, 0, 0, 0),
        ]);
        (base, over, stats)
    }

    fn assert_same(view: &SignalView<'_>, oracle: &CheapestListingsMap, item: i32) {
        let got = view.find_matching_listings(item);
        let want = oracle.find_matching_listings(item);
        assert_eq!(got.lq, want.lq, "item {item} lq");
        assert_eq!(got.hq, want.hq, "item {item} hq");
    }

    #[test]
    fn signal_view_matches_override_then_overlay_on_fixture() {
        let (base, over, st) = fixture();
        let index = stats_index(&st);
        for stat in [SaleStat::Min, SaleStat::Median, SaleStat::Avg] {
            let oracle = overlay_sale_stats(&override_listings(&base, &over), &st, stat);
            let view = SignalView { over: Some(&over), base: &base, stats: Some((&index, stat)) };
            for item in 1..=6 {
                assert_same(&view, &oracle, item);
            }
            // Cost side: no override layer.
            let oracle = overlay_sale_stats(&base, &st, stat);
            let view = SignalView { over: None, base: &base, stats: Some((&index, stat)) };
            for item in 1..=6 {
                assert_same(&view, &oracle, item);
            }
        }
        // Listing signal: override only.
        let oracle = override_listings(&base, &over);
        let view = SignalView { over: Some(&over), base: &base, stats: None };
        for item in 1..=6 {
            assert_same(&view, &oracle, item);
        }
    }

    #[test]
    fn signal_view_never_prices_a_missing_or_zero_stat_at_zero() {
        let (base, _, st) = fixture();
        let index = stats_index(&st);
        let view = SignalView { over: None, base: &base, stats: Some((&index, SaleStat::Median)) };
        // Item 5 has a zero-priced stat row: keep the listing.
        assert_eq!(view.find_matching_listings(5).lowest_gil(), Some(50));
        // Item 2 NQ has a listing but no stat row: keep the listing.
        assert_eq!(view.find_matching_listings(2).lq.map(|d| d.price), Some(200));
        // Item 4 has a stat row but no listing: gains an entry with world 0.
        let four = view.find_matching_listings(4);
        assert_eq!(four.lq.map(|d| (d.price, d.world_id)), Some((20, 0)));
        // Item 6 has nothing.
        assert_eq!(view.find_matching_listings(6).lowest_gil(), None);
    }

    #[test]
    fn stat_priced_entries_keep_the_listing_world() {
        let (base, over, st) = fixture();
        let index = stats_index(&st);
        let view = SignalView { over: Some(&over), base: &base, stats: Some((&index, SaleStat::Min)) };
        // Item 1 NQ: stat 90 wins, world from the override layer (42).
        assert_eq!(view.find_matching_listings(1).lq, Some(CheapestListingData { price: 90, world_id: 42 }));
    }

    #[test]
    fn arc_and_ref_implement_price_lookup() {
        fn takes<P: PriceLookup + ?Sized>(p: &P) -> Option<i32> {
            p.find_matching_listings(1).lowest_gil()
        }
        let (base, _, _) = fixture();
        let arc = Arc::new(base.clone());
        assert_eq!(takes(&base), Some(100));
        assert_eq!(takes(&arc), Some(100));
        assert_eq!(takes(&&base), Some(100));
    }
}
```

- [ ] **Step 2: Register the module and run the tests to verify they fail**

Add `pub mod signals;` to `analyzer_kit/mod.rs`.

Run: `cargo test -p ultros-app --lib analyzer_kit::signals`
Expected: FAIL to compile (`PriceLookup` not found).

- [ ] **Step 3: Implement**

Above `mod tests` in `signals.rs`:

```rust
use std::collections::HashMap;
use std::sync::Arc;

use ultros_api_types::cheapest_listings::{
    CheapestListingData, CheapestListingMapKey, CheapestListingsMap, PriceSummary,
};
use ultros_api_types::sale_stats::{BulkSaleStats, ItemSaleStats};

use super::formula::SaleStat;

/// The one lookup the pricing core needs. `compute_cost` is generic over
/// it, so a lazily layered view can stand in for a cloned map.
pub trait PriceLookup {
    fn find_matching_listings(&self, item_id: i32) -> PriceSummary;
}

impl PriceLookup for CheapestListingsMap {
    fn find_matching_listings(&self, item_id: i32) -> PriceSummary {
        CheapestListingsMap::find_matching_listings(self, item_id)
    }
}

impl<P: PriceLookup + ?Sized> PriceLookup for &P {
    fn find_matching_listings(&self, item_id: i32) -> PriceSummary {
        (**self).find_matching_listings(item_id)
    }
}

impl<P: PriceLookup + ?Sized> PriceLookup for Arc<P> {
    fn find_matching_listings(&self, item_id: i32) -> PriceSummary {
        (**self).find_matching_listings(item_id)
    }
}

/// Sale statistics keyed by `(item_id, hq)`, built once per payload.
pub type StatsIndex = HashMap<(i32, bool), ItemSaleStats>;

pub fn stats_index(stats: &BulkSaleStats) -> StatsIndex {
    stats.stats.iter().map(|s| ((s.item_id, s.hq), *s)).collect()
}

/// The statistic a signal reads from one row.
pub fn stat_price(row: &ItemSaleStats, stat: SaleStat) -> i32 {
    match stat {
        SaleStat::Min => row.min_price,
        SaleStat::Median => row.median_price,
        SaleStat::Avg => row.avg_price,
    }
}

/// A price view layered from three sources without copying any of them:
/// the `over` listings win where present (the sell world's own price),
/// `base` listings fill the rest (the buy scope), and when a sale statistic
/// is selected it replaces the price of every `(item, hq)` that has a
/// non-zero stat row — keeping the listing's world so "cheapest world"
/// still means something. Items with a stat but no listing gain an entry
/// with world 0; items with a zero or missing stat keep their listing;
/// nothing is ever priced at 0 because of an absent statistic.
///
/// This is exactly `overlay_sale_stats(&override_listings(base, over), ..)`
/// from `price_basis`, evaluated per lookup.
pub struct SignalView<'a> {
    pub over: Option<&'a CheapestListingsMap>,
    pub base: &'a CheapestListingsMap,
    pub stats: Option<(&'a StatsIndex, SaleStat)>,
}

impl SignalView<'_> {
    fn quality(&self, item_id: i32, hq: bool) -> Option<CheapestListingData> {
        let key = CheapestListingMapKey { item_id, hq };
        let listing = self
            .over
            .and_then(|o| o.map.get(&key).copied())
            .or_else(|| self.base.map.get(&key).copied());
        match self.stats {
            Some((index, stat)) => match index.get(&(item_id, hq)) {
                Some(row) if stat_price(row, stat) > 0 => Some(CheapestListingData {
                    price: stat_price(row, stat),
                    world_id: listing.map(|l| l.world_id).unwrap_or(0),
                }),
                _ => listing,
            },
            None => listing,
        }
    }
}

impl PriceLookup for SignalView<'_> {
    fn find_matching_listings(&self, item_id: i32) -> PriceSummary {
        PriceSummary {
            lq: self.quality(item_id, false),
            hq: self.quality(item_id, true),
        }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-app --lib analyzer_kit::signals`
Expected: PASS (4 tests).

- [ ] **Step 5: Clippy, fmt, commit**

Run: `cargo fmt --all && cargo clippy -p ultros-app --all-targets -- -D warnings`
Expected: clean except the transient dead-code items noted in Task 2 (now also `SignalView`, `stats_index`); all resolved by Task 6.

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit
git commit -m "feat(analyzer-kit): PriceLookup trait and zero-copy SignalView"
```

---

### Task 4: `compute_cost` generic over `PriceLookup`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/crafting_cost.rs:139-143, 226-243`
- Test: `ultros-frontend/ultros-app/src/components/crafting_cost.rs` (existing test module)

**Interfaces:**
- Consumes: `analyzer_kit::signals::{PriceLookup, SignalView}`.
- Produces: `compute_ingredient_cost<P: PriceLookup + ?Sized>(item_id, amount_needed, prices: &P, opts)`, `compute_cost<P: PriceLookup + ?Sized>(recipe, prices: &P, recipes_by_output, opts, is_shard)`. Every existing caller (`recipe_analyzer.rs:909` with `&Arc<CheapestListingsMap>`, `item_view.rs:994-995`, `related_items.rs:210/225/422/433`, `fc_crafting_analyzer.rs:198` with `&CheapestListingsMap`) compiles unchanged through type inference.

- [ ] **Step 1: Write the failing test** (append to the existing `mod tests` in `crafting_cost.rs`; use the fixtures the module already has — look at `fixtures.rs` for the recipe/price helpers used by `compute_cost_subcraft_termination` and reuse the same constructors)

```rust
    #[test]
    fn compute_cost_accepts_a_signal_view() {
        use crate::analyzer_kit::formula::SaleStat;
        use crate::analyzer_kit::signals::{SignalView, stats_index};
        use ultros_api_types::sale_stats::{BulkSaleStats, ItemSaleStats};

        // One-ingredient recipe: item 10 → item 20, needs 2 of item 10.
        let recipe = make_recipe_yielding(&[(10, 2)], 20, 1);
        let listings = one_listing(10, false, 100, 1);
        let stats = BulkSaleStats {
            stats: vec![ItemSaleStats {
                item_id: 10,
                hq: false,
                min_price: 40,
                median_price: 60,
                avg_price: 70,
                num_sold: 5,
                ..Default::default()
            }],
        };
        let index = stats_index(&stats);
        let empty = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &empty,
            vendor_prices: None,
        };
        let by_output = HashMap::new();
        let not_shard = |_: ItemId| false;

        let plain = compute_cost(&recipe, &listings, &by_output, &opts, &not_shard);
        assert_eq!(plain.cost, 200);

        let view = SignalView { over: None, base: &listings, stats: Some((&index, SaleStat::Median)) };
        let priced = compute_cost(&recipe, &view, &by_output, &opts, &not_shard);
        assert_eq!(priced.cost, 120);
    }
```

`make_recipe_yielding(ingredients: &[(i32, i32)], item_result: i32, yield_qty: i32) -> Recipe` and `one_listing(item_id, hq, price, world_id) -> CheapestListingsMap` are private helpers already defined in this file's `mod tests`; `crafting_cost/fixtures.rs` holds only whole-scenario fixtures and is not what this test wants.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ultros-app --lib crafting_cost::tests::compute_cost_accepts_a_signal_view`
Expected: FAIL to compile: expected `&CheapestListingsMap`, found `&SignalView`.

- [ ] **Step 3: Make the three functions generic**

Change the imports at the top of `crafting_cost.rs`:

```rust
use crate::analyzer_kit::signals::PriceLookup;
```

(keep `use ultros_api_types::cheapest_listings::CheapestListingsMap;` only if something else in the file still names it; otherwise remove it).

Change the three signatures:

```rust
pub fn compute_ingredient_cost<P: PriceLookup + ?Sized>(
    item_id: ItemId,
    amount_needed: i32,
    prices: &P,
    opts: &CraftingCostOptions<'_>,
) -> IngredientLine {
```

```rust
pub fn compute_cost<P: PriceLookup + ?Sized>(
    recipe: &Recipe,
    prices: &P,
    recipes_by_output: &HashMap<ItemId, Vec<&'static Recipe>>,
    opts: &CraftingCostOptions<'_>,
    is_shard: &dyn Fn(ItemId) -> bool,
) -> CostBreakdown {
    compute_cost_inner(recipe, prices, recipes_by_output, opts, is_shard, 0)
}

fn compute_cost_inner<P: PriceLookup + ?Sized>(
    recipe: &Recipe,
    prices: &P,
    recipes_by_output: &HashMap<ItemId, Vec<&'static Recipe>>,
    opts: &CraftingCostOptions<'_>,
    is_shard: &dyn Fn(ItemId) -> bool,
    depth: u8,
) -> CostBreakdown {
```

The bodies are unchanged: `prices.find_matching_listings(item_id.0)` resolves through the trait.

- [ ] **Step 4: Run the whole crate's tests**

Run: `cargo test -p ultros-app --lib`
Expected: PASS; in particular every `crafting_cost` test and the four callers compile (`recipe_analyzer.rs:909` passes `&prices` where `prices: Arc<CheapestListingsMap>` → `P = Arc<CheapestListingsMap>`).

- [ ] **Step 5: Clippy, fmt, commit**

Run: `cargo fmt --all && cargo clippy -p ultros-app --all-targets -- -D warnings`

```bash
git add ultros-frontend/ultros-app/src/components/crafting_cost.rs
git commit -m "refactor(crafting-cost): price lookups generic over PriceLookup"
```

---

### Task 5: `needed_bodies`, the single fetch gate

**Files:**
- Create: `ultros-frontend/ultros-app/src/analyzer_kit/needed.rs`
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs` (add `pub mod needed;`)

**Interfaces:**
- Consumes: `ProfitFormula` (Task 2).
- Produces:
  - `pub enum BodyRole { CheapestBuyScope, CheapestSellWorld, SellWorldStats(u16), BuyScopeStats(u16), RecentSalesSellWorld }` (`Ord`, so it sits in a `BTreeSet`)
  - `pub const SALE_STATS_WINDOW_DAYS: u16 = 7;` (moved from the route)
  - `pub struct RecipeNeeds { pub outliers: bool, pub buy_scope_is_sell_world: bool }`
  - `pub fn needed_bodies(formula: &ProfitFormula, needs: &RecipeNeeds) -> BTreeSet<BodyRole>`

- [ ] **Step 1: Write the failing tests**

```rust
// ultros-frontend/ultros-app/src/analyzer_kit/needed.rs
//! Which bulk bodies a view needs. The page turns each role into one
//! resource, so "what does this URL fetch" is a pure function.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer_kit::formula::{BuyScope, PriceSignal, ProfitFormula};

    fn needs(outliers: bool, same: bool) -> RecipeNeeds {
        RecipeNeeds { outliers, buy_scope_is_sell_world: same }
    }

    #[test]
    fn needed_bodies_default_is_todays_three_bodies() {
        let f = ProfitFormula::recipe_from_query(None, None, None);
        let got = needed_bodies(&f, &needs(false, false));
        assert_eq!(
            got.into_iter().collect::<Vec<_>>(),
            vec![
                BodyRole::CheapestBuyScope,
                BodyRole::CheapestSellWorld,
                BodyRole::SellWorldStats(SALE_STATS_WINDOW_DAYS),
            ]
        );
    }

    #[test]
    fn sale_cost_signal_adds_the_buy_scope_stats_body() {
        let f = ProfitFormula::recipe_from_query(Some(PriceSignal::SaleMedian), None, None);
        let got = needed_bodies(&f, &needs(false, false));
        assert!(got.contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS)));
        // A sale REVENUE signal reads the sell-world body, already present.
        let f = ProfitFormula::recipe_from_query(None, Some(PriceSignal::SaleMin), None);
        let got = needed_bodies(&f, &needs(false, false));
        assert!(!got.contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS)));
    }

    #[test]
    fn needed_bodies_dedupes_buy_scope_equal_to_sell_world() {
        let f = ProfitFormula::recipe_from_query(
            Some(PriceSignal::SaleMedian),
            None,
            Some(BuyScope::World),
        );
        let got = needed_bodies(&f, &needs(false, true));
        assert!(!got.contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS)));
    }

    #[test]
    fn outlier_filter_needs_recent_sales() {
        let f = ProfitFormula::recipe_from_query(None, None, None);
        assert!(needed_bodies(&f, &needs(true, false)).contains(&BodyRole::RecentSalesSellWorld));
        assert!(!needed_bodies(&f, &needs(false, false)).contains(&BodyRole::RecentSalesSellWorld));
    }
}
```

- [ ] **Step 2: Register and run to verify failure**

Add `pub mod needed;` to `mod.rs`. Run: `cargo test -p ultros-app --lib analyzer_kit::needed`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
use std::collections::BTreeSet;

use super::formula::{BuyScope, ProfitFormula};

/// The one sale-history window every recipe-analyzer body uses. The
/// server serves 1 | 7 | 30 | 90; the labels in seven locales say "(7d)".
pub const SALE_STATS_WINDOW_DAYS: u16 = 7;

/// A whole-scope body the page fetches. Symbolic: the page resolves each
/// role to a world / datacenter / region name.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BodyRole {
    CheapestBuyScope,
    CheapestSellWorld,
    SellWorldStats(u16),
    BuyScopeStats(u16),
    RecentSalesSellWorld,
}

/// Page state that changes which bodies are needed but is not part of
/// the formula.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct RecipeNeeds {
    /// The opt-in outlier filter reads raw recent sales.
    pub outliers: bool,
    /// Buy from = This world only, and it resolved to the sell world: the
    /// sell-world stats body doubles as the buy-scope body.
    pub buy_scope_is_sell_world: bool,
}

/// The bodies the recipe analyzer needs for `formula`. The default URL
/// yields exactly the three bodies the page fetches today.
pub fn needed_bodies(formula: &ProfitFormula, needs: &RecipeNeeds) -> BTreeSet<BodyRole> {
    let mut set = BTreeSet::from([
        BodyRole::CheapestBuyScope,
        BodyRole::CheapestSellWorld,
        BodyRole::SellWorldStats(SALE_STATS_WINDOW_DAYS),
    ]);
    // The buy-scope body aliases the sell-world body only when the scope
    // IS a world and that world resolved to the sell world.
    let aliased = formula.buy_scope() == BuyScope::World && needs.buy_scope_is_sell_world;
    if formula.cost_signal().sale_stat().is_some() && !aliased {
        set.insert(BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS));
    }
    if needs.outliers {
        set.insert(BodyRole::RecentSalesSellWorld);
    }
    set
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-app --lib analyzer_kit::needed`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-app/src/analyzer_kit
git commit -m "feat(analyzer-kit): needed_bodies fetch gate"
```

---

### Task 6: Extract `price_rows` and `filter_and_sort`, then re-price through `SignalView`

This is the load-bearing task. It is done in three moves so the characterization test is recorded against the unchanged pipeline before anything is refactored.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (the table component 611-1000 and its test module)

**Interfaces:**
- Consumes: `ProfitFormula`, `profit_line`, `per_unit_cost`, `effective` (Task 2); `SignalView`, `StatsIndex`, `stats_index` (Task 3); generic `compute_cost` (Task 4).
- Produces (all private to the route):
  - `struct PriceInputs<'a>` (see Step 3)
  - `fn price_rows(inp: &PriceInputs<'_>) -> Vec<RecipeProfitData>`
  - `struct Thresholds { min_profit: Option<i32>, min_roi: Option<i32>, min_daily_sales: Option<f32>, listing_world: Option<String>, listing_dc: Option<String> }`
  - `fn filter_and_sort(rows: &[Arc<RecipeProfitData>], t: &Thresholds, world_names: &HashMap<i32, (String, String)>, mode: SortMode, dir: SortDir) -> Vec<(usize, Arc<RecipeProfitData>)>`
  - `RecipeProfitData` unchanged.

- [ ] **Step 1: Move A — extract the loop verbatim into `price_rows`**

Add above `#[component] fn RecipeAnalyzerTable` (after `compare_recipes`):

```rust
/// Everything the pricing pass reads, snapshotted out of the reactive
/// graph so the pass is a plain function (and unit-testable).
struct PriceInputs<'a> {
    recipes: &'a [&'static Recipe],
    recipe_level_tables: &'static HashMap<RecipeLevelTableId, xiv_gen::RecipeLevelTable>,
    recipes_by_output: &'a HashMap<ItemId, Vec<&'static Recipe>>,
    /// Buy-scope listings.
    buy_listings: &'a CheapestListingsMap,
    /// Sell-world listings (absent before a world resolves).
    sell_listings: Option<&'a CheapestListingsMap>,
    /// Buy-scope sale stats, indexed. `None` when not fetched.
    buy_stats: Option<&'a StatsIndex>,
    /// Sell-world sale stats, indexed. Empty when not fetched.
    sell_stats: &'a StatsIndex,
    /// Raw recent sales by item (both qualities merged), for the outlier
    /// filter and the rollup failover.
    raw_sales: &'a HashMap<i32, Vec<&'a SaleData>>,
    formula: ProfitFormula,
    levels: &'a CrafterLevels,
    job_filter: Option<&'a str>,
    use_subcrafts: bool,
    require_hq: bool,
    filter_outliers: bool,
    shards: ShardsMode,
    /// The on-hand stockpile when the on-hand toggle is on.
    on_hand: Option<&'a HashMap<i32, i32>>,
}

/// One priced row per craftable recipe with a sell price, under the
/// selected formula. Unprofitable rows are dropped here (the formula's
/// [`DropRule`]); thresholds and sorting happen in [`filter_and_sort`].
fn price_rows(inp: &PriceInputs<'_>) -> Vec<RecipeProfitData> {
    // MOVE A: paste the body of the old `computed_data` memo from the line
    // `let mut results = Vec::new();` down to (not including) the line
    // `// Filter results`, then apply exactly these substitutions and
    // nothing else:
    //   recipes.values()                      -> inp.recipes.iter().copied()
    //   recipe_level_tables                   -> inp.recipe_level_tables
    //   recipes_by_output                     -> inp.recipes_by_output
    //   &levels                               -> inp.levels
    //   job_filter() / `if let Some(filter) = job_filter()` -> inp.job_filter
    //   use_sub                               -> inp.use_subcrafts
    //   require_hq_flag                       -> inp.require_hq
    //   filter_outliers                       -> inp.filter_outliers
    //   sales_map                             -> inp.raw_sales
    //   sell_stats_map                        -> inp.sell_stats
    //   shards                                -> inp.shards
    //   use_on_hand / on_hand_map             -> see the on-hand block below
    //   &prices (compute_cost argument)       -> &ingredient_view
    //   revenue.find_matching_listings(..)    -> revenue_view.find_matching_listings(..)
    //   raw_prices.find_matching_listings(..) -> inp.buy_listings.find_matching_listings(..)
    //   `if !has_levels() { return vec![]; }` -> `if !has_any_level(inp.levels) { return Vec::new(); }`
    // The two views are built once, before the loop:
    let ingredient_view = SignalView {
        over: None,
        base: inp.buy_listings,
        stats: inp
            .formula
            .cost_signal()
            .sale_stat()
            .and_then(|stat| inp.buy_stats.map(|idx| (idx, stat))),
    };
    let revenue_view = SignalView {
        over: inp.sell_listings,
        base: inp.buy_listings,
        stats: inp
            .formula
            .revenue_signal()
            .sale_stat()
            .map(|stat| (inp.sell_stats, stat)),
    };
    // On-hand: one snapshot source; the per-recipe `LocalOnHand` clone stays
    // inside the loop ONLY when on-hand is on (compute_cost consumes it).
    //   let active: Box<dyn OnHand> = match inp.on_hand {
    //       Some(map) => Box::new(LocalOnHand::from_map(map.clone())),
    //       None => Box::new(EmptyOnHand),
    //   };
    // Profit math: replace the block from `let net_revenue = ...` through the
    // `let roi = ...` expression with
    //   let (line, dropped) = profit_line(market_price, cost_per_unit, &inp.formula);
    //   if dropped { continue; }
    // and use `line.profit`, `line.roi`, `line.tax` in the pushed struct
    // (`tax: line.tax` replaces `tax: market_price - net_revenue`).
    todo!("replace this line with the moved loop; see the substitutions above")
}
```

Then write the actual function body by performing the paste and substitutions. The result must read like this skeleton (abbreviated where the old code is unchanged):

```rust
fn price_rows(inp: &PriceInputs<'_>) -> Vec<RecipeProfitData> {
    let mut results = Vec::new();
    if !has_any_level(inp.levels) {
        return results;
    }
    let ingredient_view = SignalView { /* as above */ };
    let revenue_view = SignalView { /* as above */ };

    for recipe in inp.recipes.iter().copied() {
        let required_level = inp
            .recipe_level_tables
            .get(&RecipeLevelTableId(recipe.recipe_level_table))
            .map(|t| t.class_job_level as i32)
            .unwrap_or(0);
        let job_code = craft_type_acronym(recipe.craft_type);
        let user_level = level_for_job_code(inp.levels, job_code).unwrap_or(0);
        if let Some(filter) = inp.job_filter
            && filter != job_code
        {
            continue;
        }
        if user_level == 0 {
            continue;
        }
        if required_level > 0 && user_level < required_level {
            continue;
        }

        let sales_stats = if inp.filter_outliers {
            inp.raw_sales
                .get(&recipe.item_result)
                .map(|sales| analyze_sales(sales, true))
        } else {
            sales_stats_from_rollup(inp.sell_stats, recipe.item_result).or_else(|| {
                inp.raw_sales
                    .get(&recipe.item_result)
                    .map(|sales| analyze_sales(sales, false))
            })
        }
        .unwrap_or(SalesStats { daily_sales: 0.0, avg_price: 0, total_sales: 0 });

        let market_price = revenue_view
            .find_matching_listings(recipe.item_result)
            .lowest_gil()
            .unwrap_or(0);
        if market_price == 0 {
            continue;
        }

        let scope_summary = inp.buy_listings.find_matching_listings(recipe.item_result);
        let cheapest_world_id = scope_summary
            .lq
            .map(|d| d.world_id)
            .or(scope_summary.hq.map(|d| d.world_id))
            .unwrap_or(0);

        let active: Box<dyn crate::components::crafting_cost::OnHand> = match inp.on_hand {
            Some(map) => Box::new(LocalOnHand::from_map(map.clone())),
            None => Box::new(EmptyOnHand),
        };
        let opts = CraftingCostOptions {
            require_hq: inp.require_hq,
            max_subcraft_depth: if inp.use_subcrafts { 2 } else { 0 },
            shards: inp.shards,
            on_hand: active.as_ref(),
            vendor_prices: Some(vendor_price_map()),
        };
        let breakdown = compute_cost(recipe, &ingredient_view, inp.recipes_by_output, &opts, &is_shard_item);
        let cost_per_unit = per_unit_cost(breakdown.cost, recipe.amount_result);

        let (line, dropped) = profit_line(market_price, cost_per_unit, &inp.formula);
        if dropped {
            continue;
        }

        let sell_stat = inp
            .sell_stats
            .get(&(recipe.item_result, inp.require_hq))
            .or_else(|| inp.sell_stats.get(&(recipe.item_result, !inp.require_hq)));
        let vwap = sell_stat.map(|s| s.vwap).unwrap_or(0);

        results.push(RecipeProfitData {
            recipe,
            profit: line.profit,
            return_on_investment: line.roi,
            cost: cost_per_unit,
            market_price,
            cheapest_world_id,
            sub_crafts: breakdown.sub_crafts,
            daily_sales: sales_stats.daily_sales,
            avg_price: sales_stats.avg_price,
            total_sales: sales_stats.total_sales,
            required_level,
            last_sold_unix: sell_stat.map(|s| s.last_sold_unix).unwrap_or(0),
            units_sold: sell_stat.map(|s| s.units_sold).unwrap_or(0),
            vwap,
            vwap_pct: vwap_pct(market_price, vwap),
            tax: line.tax,
            confidence: sell_stat.map(|s| s.confidence).unwrap_or_default(),
        });
    }
    results
}
```

Note the two behaviour-preserving simplifications that are NOT number changes: the on-hand map is cloned only when on-hand is on (today it is cloned per recipe regardless and then ignored), and `breakdown.sub_crafts` is moved instead of cloned.

Then add the filter/sort function:

```rust
/// The user's row filters. `None` = not set.
#[derive(Clone, Debug, PartialEq, Default)]
struct Thresholds {
    min_profit: Option<i32>,
    min_roi: Option<i32>,
    min_daily_sales: Option<f32>,
    listing_world: Option<String>,
    listing_dc: Option<String>,
}

/// Apply the thresholds and sort. Pure, so a header click never re-prices.
fn filter_and_sort(
    rows: &[Arc<RecipeProfitData>],
    t: &Thresholds,
    world_names: &HashMap<i32, (String, String)>,
    mode: SortMode,
    dir: SortDir,
) -> Vec<(usize, Arc<RecipeProfitData>)> {
    let mut kept: Vec<Arc<RecipeProfitData>> = rows
        .iter()
        .filter(|d| t.min_profit.is_none_or(|min| d.profit >= min))
        .filter(|d| t.min_roi.is_none_or(|min| d.return_on_investment >= min))
        .filter(|d| t.min_daily_sales.is_none_or(|min| d.daily_sales >= min))
        .filter(|d| {
            if t.listing_world.is_none() && t.listing_dc.is_none() {
                return true;
            }
            listing_location_passes(
                world_names.get(&d.cheapest_world_id),
                t.listing_world.as_deref(),
                t.listing_dc.as_deref(),
            )
        })
        .cloned()
        .collect();
    kept.sort_by(|a, b| {
        let ord = match dir {
            SortDir::Asc => compare_recipes(mode, a, b),
            SortDir::Desc => compare_recipes(mode, a, b).reverse(),
        };
        // Deterministic tiebreak: the input comes from a std HashMap, so
        // without it ties could order differently on the server and the
        // client and mismatch the 19 SSR-rendered rows.
        ord.then_with(|| a.recipe.key_id.0.cmp(&b.recipe.key_id.0))
    });
    kept.into_iter().enumerate().collect()
}
```

`kept.sort_by` (stable) replaces `sort_unstable_by`; with the tiebreak the order is total, so stability does not change results.

- [ ] **Step 2: Move A — rewire the memos**

Replace lines 735-993 (`has_levels`, `ingredient_prices`, `revenue_prices`, `raw_prices`, `sell_stats_for_rows`, `world_names_for_rows` and `computed_data`) with:

```rust
    // Indexes are built once per payload, not once per recompute.
    let sell_stats_index: Arc<StatsIndex> = Arc::new(stats_index(&sell_world_sale_stats));
    let buy_stats_index: Option<Arc<StatsIndex>> = buy_stats_loaded.then(|| Arc::new(stats_index(&sale_stats)));
    let all_recipes: Arc<Vec<&'static Recipe>> = Arc::new(recipes.values().collect());

    let formula = Memo::new(move |_| {
        ProfitFormula::recipe_from_query(cost_basis(), revenue_metric(), buy_scope())
            .effective(buy_stats_loaded, sell_stats_loaded)
    });

    let on_hand_map = use_context::<OnHandMap>();
    let priced: Memo<Arc<Vec<Arc<RecipeProfitData>>>> = {
        let prices = prices.clone();
        let sell_world_prices = sell_world_prices.clone();
        let sell_stats_index = sell_stats_index.clone();
        let buy_stats_index = buy_stats_index.clone();
        let all_recipes = all_recipes.clone();
        Memo::new(move |_| {
            let raw_sales: HashMap<i32, Vec<&SaleData>> = recent_sales
                .as_ref()
                .map(|sales| {
                    let mut map: HashMap<i32, Vec<&SaleData>> = HashMap::new();
                    for sale in &sales.sales {
                        map.entry(sale.item_id).or_default().push(sale);
                    }
                    map
                })
                .unwrap_or_default();
            let levels = crafter_levels.get().unwrap_or_default();
            let job = job_filter();
            let on_hand = use_on_hand_enabled()
                .then(|| on_hand_map.map(|m| m.0.get_untracked()).unwrap_or_default());
            let recipes_by_output = recipes_by_output();
            let inp = PriceInputs {
                recipes: &all_recipes,
                recipe_level_tables,
                recipes_by_output: &recipes_by_output,
                buy_listings: &prices,
                sell_listings: sell_world_prices.as_deref(),
                buy_stats: buy_stats_index.as_deref(),
                sell_stats: &sell_stats_index,
                raw_sales: &raw_sales,
                formula: formula(),
                levels: &levels,
                job_filter: job.as_deref(),
                use_subcrafts: use_subcrafts().unwrap_or(false),
                require_hq: require_hq().unwrap_or(false),
                filter_outliers: filter_outliers().unwrap_or(false),
                shards: if exclude_shards_enabled() {
                    ShardsMode::ExcludeShards
                } else {
                    ShardsMode::IncludeMarket
                },
                on_hand: on_hand.as_ref(),
            };
            Arc::new(price_rows(&inp).into_iter().map(Arc::new).collect())
        })
    };

    let world_names_for_rows = world_names.clone();
    let computed_data = Memo::new(move |_| {
        let t = Thresholds {
            min_profit: minimum_profit(),
            min_roi: minimum_roi(),
            min_daily_sales: min_daily_sales(),
            listing_world: listing_world_filter(),
            listing_dc: listing_dc_filter(),
        };
        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = sort_dir().unwrap_or_else(|| mode.default_dir());
        filter_and_sort(&priced(), &t, &world_names_for_rows, mode, dir)
    });
```

Supporting edits in the same component:
- The props `sale_stats: Option<BulkSaleStats>` and `sell_world_sale_stats: Option<BulkSaleStats>` stay; derive `let buy_stats_loaded = sale_stats.is_some(); let sell_stats_loaded = sell_world_sale_stats.is_some();` BEFORE the existing `let sale_stats = Arc::new(sale_stats.unwrap_or_default());` lines, and drop the `Arc` wrapping of the two `BulkSaleStats` (they are only read once by `stats_index`).
- `recipes_by_output` stays a `Memo` (it is built once per mount; reading it inside `priced` is fine).
- Delete `raw_prices`, `sell_stats_for_rows`, `has_levels` (the `empty_reason` memo uses `crafter_levels` directly: replace `has_levels()`-based logic with `has_any_level(&crafter_levels.get().unwrap_or_default())` where the old memo was read).
- Imports: replace the Task 2 line with `use crate::analyzer_kit::formula::{ProfitFormula, per_unit_cost, profit_line};` (`TaxMath` and `net_after_tax` no longer have a route consumer), add `use crate::analyzer_kit::signals::{PriceLookup, SignalView, StatsIndex, stats_index};`, and remove `overlay_sale_stats, override_listings` from the `price_basis` import. `PriceLookup` must be in scope for `revenue_view.find_matching_listings`.
- `computed_data` keeps its type `Memo<Vec<(usize, Arc<RecipeProfitData>)>>`, so every downstream reader (`empty_state`, the VirtualScroller `each`, the result count) is untouched.

- [ ] **Step 3: Build, then run the existing tests**

Run: `cargo test -p ultros-app --lib recipe_analyzer`
Expected: PASS (the 20 existing tests). Run `cargo fmt --all && cargo clippy -p ultros-app --all-targets -- -D warnings`; the transient dead-code items from Tasks 2 and 3 must now be gone because `price_rows` consumes them.

- [ ] **Step 4: Commit Move A**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "refactor(recipe-analyzer): extract price_rows and filter_and_sort, price through SignalView"
```

- [ ] **Step 5: Write the characterization tests** (append to `mod test` in `recipe_analyzer.rs`)

The fixture uses real game data (the test module already calls `xiv_gen_db::data()`), a synthetic price map that prices every ingredient of the first 300 recipes deterministically, and hand-verified expectations for three of them.

```rust
    use crate::analyzer_kit::formula::{PriceSignal, ProfitFormula};
    use crate::analyzer_kit::signals::stats_index;
    use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};
    use ultros_api_types::sale_stats::{BulkSaleStats, ItemSaleStats};

    /// Deterministic synthetic market: every item `i` lists NQ at
    /// `100 + (i % 97) * 7` on world 1 and HQ at that plus 50 on world 2;
    /// the sell world lists the OUTPUT items of the fixture recipes 20%
    /// higher on world 3; 7d stats exist for every third item.
    fn fixture(recipes: &[&'static Recipe]) -> (CheapestListingsMap, CheapestListingsMap, BulkSaleStats) {
        let mut buy = Vec::new();
        let mut sell = Vec::new();
        let mut stats = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for r in recipes {
            for id in r.ingredient.iter().chain(std::iter::once(&r.item_result)) {
                if *id == 0 || !seen.insert(*id) {
                    continue;
                }
                let nq = 100 + (*id % 97) * 7;
                buy.push(CheapestListingItem { item_id: *id, hq: false, cheapest_price: nq, world_id: 1 });
                buy.push(CheapestListingItem { item_id: *id, hq: true, cheapest_price: nq + 50, world_id: 2 });
                if *id % 3 == 0 {
                    stats.push(ItemSaleStats {
                        item_id: *id,
                        hq: false,
                        min_price: nq - 10,
                        median_price: nq + 5,
                        avg_price: nq + 9,
                        num_sold: 14,
                        ..Default::default()
                    });
                }
            }
            let out = r.item_result;
            let nq = 100 + (out % 97) * 7;
            sell.push(CheapestListingItem { item_id: out, hq: false, cheapest_price: nq * 12 / 10, world_id: 3 });
        }
        (
            CheapestListingsMap::from(CheapestListings { cheapest_listings: buy }),
            CheapestListingsMap::from(CheapestListings { cheapest_listings: sell }),
            BulkSaleStats { stats },
        )
    }

    fn fixture_recipes() -> Vec<&'static Recipe> {
        let data = xiv_gen_db::data();
        let mut all: Vec<&'static Recipe> = data.recipes.values().collect();
        all.sort_by_key(|r| r.key_id.0);
        all.into_iter().take(300).collect()
    }

    fn run(cost: PriceSignal, revenue: PriceSignal, outliers: bool) -> Vec<RecipeProfitData> {
        let data = xiv_gen_db::data();
        let recipes = fixture_recipes();
        let (buy, sell, stats) = fixture(&recipes);
        let index = stats_index(&stats);
        let by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let raw_sales = HashMap::new();
        let levels = CrafterLevels::default(); // 100 in every job
        let inp = PriceInputs {
            recipes: &recipes,
            recipe_level_tables: &data.recipe_level_tables,
            recipes_by_output: &by_output,
            buy_listings: &buy,
            sell_listings: Some(&sell),
            buy_stats: Some(&index),
            sell_stats: &index,
            raw_sales: &raw_sales,
            formula: ProfitFormula::recipe_from_query(Some(cost), Some(revenue), None),
            levels: &levels,
            job_filter: None,
            use_subcrafts: false,
            require_hq: false,
            filter_outliers: outliers,
            shards: ShardsMode::ExcludeShards,
            on_hand: None,
        };
        price_rows(&inp)
    }

    /// Every row obeys the formula's arithmetic and the drop rule; this
    /// runs over 300 real recipes with synthetic prices.
    #[test]
    fn price_rows_rows_obey_the_formula() {
        let rows = run(PriceSignal::ListingMin, PriceSignal::ListingMin, false);
        assert!(rows.len() > 50, "fixture priced only {} rows", rows.len());
        for r in &rows {
            let net = r.market_price as i64 * 95 / 100;
            assert!((r.cost as i64) < net, "row kept with cost >= net: {:?}", r.recipe.key_id);
            assert_eq!(r.profit as i64, net - r.cost as i64);
            assert_eq!(r.tax as i64, r.market_price as i64 - net);
            let roi = if r.cost > 0 { (r.profit as f64 / r.cost as f64 * 100.0) as i32 } else { 0 };
            assert_eq!(r.return_on_investment, roi);
            // Revenue is `lowest_gil()` over the sell world's NQ listing (20% up)
            // and the buy scope's HQ listing (`nq + 50`), whichever is lower:
            // exactly today's `override_listings` + `lowest_gil` behaviour.
            let nq = 100 + (r.recipe.item_result % 97) * 7;
            assert_eq!(r.market_price, (nq * 12 / 10).min(nq + 50));
        }
    }

    /// The characterization oracle. Regenerate ONLY if a phase changes the
    /// numbers on purpose: run with `--nocapture`, copy the printed tuples.
    #[test]
    fn price_rows_matches_recorded_oracle_on_fixture() {
        let rows = run(PriceSignal::SaleMedian, PriceSignal::ListingMin, false);
        let got: Vec<(i32, i32, i32, i32, i32, i32)> = rows
            .iter()
            .take(12)
            .map(|r| (r.recipe.key_id.0, r.profit, r.return_on_investment, r.cost, r.market_price, r.tax))
            .collect();
        println!("ORACLE = {got:?}");
        // Recorded from the pre-refactor pipeline (Move A, commit above).
        const ORACLE: &[(i32, i32, i32, i32, i32, i32)] = &[
            // paste the printed tuples here
        ];
        assert_eq!(got, ORACLE);
    }
```

- [ ] **Step 6: Record the oracle**

Run: `cargo test -p ultros-app --lib price_rows_matches_recorded_oracle_on_fixture -- --nocapture`
Expected: the test FAILS (empty `ORACLE`) and prints `ORACLE = [...]`. Paste the printed slice into the `ORACLE` constant. Re-run:

Run: `cargo test -p ultros-app --lib recipe_analyzer::test::price_rows`
Expected: PASS (2 tests).

- [ ] **Step 7: Filter-and-sort tests**

```rust
    fn row(key: i32, profit: i32, roi: i32, daily: f32, world: i32) -> Arc<RecipeProfitData> {
        let recipe = fixture_recipes()
            .into_iter()
            .find(|r| r.key_id.0 == key)
            .expect("fixture recipe");
        Arc::new(RecipeProfitData {
            recipe,
            profit,
            return_on_investment: roi,
            cost: 1,
            market_price: 2,
            cheapest_world_id: world,
            sub_crafts: vec![],
            daily_sales: daily,
            avg_price: 0,
            total_sales: 0,
            required_level: 1,
            last_sold_unix: 0,
            units_sold: 0,
            vwap: 0,
            vwap_pct: None,
            tax: 0,
            confidence: ConfidenceBand::Unknown,
        })
    }

    #[test]
    fn filter_and_sort_is_pure_and_inclusive() {
        let keys: Vec<i32> = fixture_recipes().iter().take(4).map(|r| r.key_id.0).collect();
        let rows = vec![
            row(keys[0], 100, 10, 1.0, 7),
            row(keys[1], 300, 30, 0.5, 8),
            row(keys[2], 200, 20, 2.0, 7),
            row(keys[3], 200, 5, 3.0, 9),
        ];
        let names: HashMap<i32, (String, String)> = [
            (7, ("Gilgamesh".to_string(), "Aether".to_string())),
            (8, ("Balmung".to_string(), "Crystal".to_string())),
        ]
        .into_iter()
        .collect();
        let t = Thresholds { min_profit: Some(200), ..Default::default() };
        let out = filter_and_sort(&rows, &t, &names, SortMode::Profit, SortDir::Desc);
        // Inclusive `>=`; ties broken by key id ascending; indexes renumbered.
        let got: Vec<(usize, i32, i32)> = out.iter().map(|(i, r)| (*i, r.profit, r.recipe.key_id.0)).collect();
        assert_eq!(got, vec![(0, 300, keys[1]), (1, 200, keys[2]), (2, 200, keys[3])]);
        // Ascending flips the order but keeps the same tiebreak direction.
        let out = filter_and_sort(&rows, &t, &names, SortMode::Profit, SortDir::Asc);
        assert_eq!(out[0].1.profit, 200);
        assert_eq!(out[0].1.recipe.key_id.0, keys[2]);
        // A listing-world filter drops unknown worlds (9 has no name).
        let t = Thresholds { listing_world: Some("Gilgamesh".into()), ..Default::default() };
        let out = filter_and_sort(&rows, &t, &names, SortMode::Profit, SortDir::Desc);
        assert_eq!(out.len(), 2);
    }
```

Run: `cargo test -p ultros-app --lib filter_and_sort_is_pure_and_inclusive`
Expected: PASS.

- [ ] **Step 8: Move B — retire the map-cloning helpers**

`override_listings` and `overlay_sale_stats` now have no non-test callers. In `price_basis.rs` put `#[cfg(test)]` on both functions and on the two `use ultros_api_types::...` lines they need; keep their tests. `SaleStat` is now only named by the test-gated `overlay_sale_stats`, so change the re-export to `pub use crate::analyzer_kit::formula::{BuyScope, CostBasis, RevenueMetric};` and add a `#[cfg(test)]`-gated `use crate::analyzer_kit::formula::SaleStat;` beside the other `#[cfg(test)]` imports. Run: `cargo clippy -p ultros-app --all-targets -- -D warnings` — clean.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-app/src/price_basis.rs
git commit -m "test(recipe-analyzer): characterization oracle for price_rows; retire map-cloning helpers"
```

---

### Task 7: Page-level `?sort=`, the folded sell-history resource, stale comments

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (page component 1873-2205, table props 611-640, the `MarketMenu` comment at 402-411)

**Interfaces:**
- Consumes: `get_sale_stats`, `get_recent_sales_for_world` (`api.rs`), `SALE_STATS_WINDOW_DAYS` (Task 5, replaces the route's own const).
- Produces: `struct SellHistory { stats: Option<BulkSaleStats>, raw: Option<RecentSales>, stats_failed: bool, raw_failed: bool }` (serde-derived: `ArcResource` values go through `JsonSerdeCodec`); table props `sort_mode: Memo<Option<SortMode>>`, `sort_dir: Memo<Option<SortDir>>`; the join passes `history.raw` / `history.stats` through the existing `recent_sales` / `sell_world_sale_stats` props.

- [ ] **Step 1: Write the failing test** (pure key logic, appended to `mod test`)

```rust
    #[test]
    fn sell_history_key_reads_outliers_not_resource_state() {
        assert_eq!(sell_history_key(Some("Gilgamesh"), false), Some(("Gilgamesh".to_string(), false)));
        assert_eq!(sell_history_key(Some("Gilgamesh"), true), Some(("Gilgamesh".to_string(), true)));
        assert_eq!(sell_history_key(None, true), None);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ultros-app --lib sell_history_key`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the folded resource**

Add near the other free functions:

```rust
/// One sell-world history payload: the 7-day rollup plus, when the outlier
/// filter is on or the rollup failed, the raw recent sales.
// `ArcResource` values round-trip through `JsonSerdeCodec`, so serde is
// required (both field types already derive it).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct SellHistory {
    stats: Option<BulkSaleStats>,
    raw: Option<RecentSales>,
    stats_failed: bool,
    raw_failed: bool,
}

/// The resource key. Deliberately built from URL state only — the old
/// key read the rollup resource inside a memo, which Leptos flags at
/// hydration (#1248 follow-up).
fn sell_history_key(world: Option<&str>, outliers: bool) -> Option<(String, bool)> {
    world.map(|w| (w.to_string(), outliers))
}

async fn fetch_sell_history(world: String, outliers: bool) -> SellHistory {
    // With the outlier filter on both bodies are needed: fetch them
    // concurrently, as the two separate resources did before the fold.
    // Otherwise the raw sales are only a failover for a failed rollup.
    let (stats, raw) = if outliers {
        let (s, r) = futures::join!(
            get_sale_stats(&world, SALE_STATS_WINDOW_DAYS),
            get_recent_sales_for_world(&world)
        );
        (s, Some(r))
    } else {
        let s = get_sale_stats(&world, SALE_STATS_WINDOW_DAYS).await;
        let r = if s.is_err() {
            Some(get_recent_sales_for_world(&world).await)
        } else {
            None
        };
        (s, r)
    };
    SellHistory {
        stats_failed: stats.is_err(),
        stats: stats.ok(),
        raw_failed: matches!(raw, Some(Err(_))),
        raw: raw.and_then(|r| r.ok()),
    }
}
```

In `RecipeAnalyzer`, delete the `sell_world_sale_stats`, `recent_sales_source` and `recent_sales` resources (lines 2047-2078) and add:

```rust
    let sell_history_source = Memo::new(move |_| {
        sell_history_key(
            selected_world.get().map(|w| w.name).as_deref(),
            filter_outliers().unwrap_or(false),
        )
    });
    let sell_history = ArcResource::new(sell_history_source, move |key: Option<(String, bool)>| async move {
        match key {
            Some((world, outliers)) => Some(fetch_sell_history(world, outliers).await),
            None => None,
        }
    });
```

Hoist the sort signals to the page (they were inside the table at 667-668):

```rust
    let (sort_mode, _) = query_signal::<SortMode>("sort");
    let (sort_dir, _) = query_signal::<SortDir>("dir");
```

Update the ToolHeader's inline error (it read `recent_sales_clone`):

```rust
                    <Suspense fallback=InlineStatusSkeleton>
                        {move || {
                            sell_history_for_header
                                .get()
                                .flatten()
                                .filter(|h| h.raw_failed)
                                .map(|_| view! { <div class="text-red-400 text-sm">{t!(i18n, error_loading_sales_data)}</div> })
                        }}
                    </Suspense>
```

with `let sell_history_for_header = sell_history.clone();` declared before the view. Delete the old `let recent_sales_clone = recent_sales.clone();` that fed this slot, and in the join drop `let sales = recent_sales.get();` and the `let recent_sales = sales.and_then(..)` line the old `match` used.

Update the Suspense join:

```rust
                    {move || {
                        let listings = global_cheapest_listings.get();
                        let stats = sale_stats.get();
                        let sell_listings = sell_world_listings.get();
                        let history = sell_history.get();
                        match (listings, stats, sell_listings, history) {
                            (Some(Ok(listings)), Some(stats), Some(sell_listings), Some(history)) => {
                                let (sale_stats, buy_stats_error) = match stats {
                                    Ok(stats) => (stats, false),
                                    Err(_) => (None, true),
                                };
                                let history = history.unwrap_or(SellHistory {
                                    stats: None,
                                    raw: None,
                                    stats_failed: false,
                                    raw_failed: false,
                                });
                                let sale_stats_error = buy_stats_error || history.stats_failed;
                                view! {
                                    <RecipeAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=history.raw
                                        sale_stats=sale_stats
                                        sell_world_sale_stats=history.stats
                                        sale_stats_error=sale_stats_error
                                        sell_world_listings=sell_listings.ok().flatten()
                                        world=Signal::derive(buy_scope_name)
                                        visible_cols=visible_cols
                                        set_cols_param=set_cols_param
                                        sort_mode=sort_mode
                                        sort_dir=sort_dir
                                    />
                                }.into_any()
                            }
                            (Some(Err(e)), _, _, _) => { /* unchanged */ }
                            _ => { /* unchanged */ }
                        }
                    }}
```

In `RecipeAnalyzerTable` add the two props and delete the two `query_signal` lines:

```rust
    sort_mode: Memo<Option<SortMode>>,
    sort_dir: Memo<Option<SortDir>>,
```

Replace the route's `const SALE_STATS_WINDOW_DAYS: u16 = 7;` with `use crate::analyzer_kit::needed::SALE_STATS_WINDOW_DAYS;`. Wire `needed_bodies` in the page as the gate for the buy-scope stats resource so the kit function has its consumer:

```rust
    let buy_sale_stats_scope = Memo::new(move |_| {
        let formula = ProfitFormula::recipe_from_query(cost_basis(), None, buy_scope());
        // Phase A is fetch-identical to `main`: the dedupe of a buy scope
        // that equals the sell world is Phase D's, so the flag stays false.
        let needs = RecipeNeeds { outliers: false, buy_scope_is_sell_world: false };
        needed_bodies(&formula, &needs)
            .contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS))
            .then_some(buy_scope_name.get())
    });
```

with `use crate::analyzer_kit::needed::{BodyRole, RecipeNeeds, needed_bodies};`. This is today's predicate (`cost_basis().unwrap_or_default().sale_stat().is_some()`) expressed through the kit; the request set for every URL is unchanged. Do NOT dedupe the buy-scope body when it equals the sell world in this phase: skipping that fetch makes `buy_stats_loaded` false in the table, and `effective()` would silently downgrade the cost signal to the listing, changing every ingredient cost under `?buy-scope=world&cost-basis=sale-*`. That dedupe is Phase D's, where the table learns that the buy body can alias the sell body.

- [ ] **Step 4: Fix the stale comments**

- Lines 402-411 (`ADDABLE_FILTERS` comment): replace "the always-visible `Pricing` button" with "the always-visible `Market` button".
- Lines 629-633 (the `visible_cols` prop doc): replace with "Visible optional columns (`?cols=`), owned by the parent because the table remounts whenever its resources change."
- Lines 1893-1895 (the `?cols=` comment in the page): replace with "`?cols=` lives here rather than in the table because the table remounts whenever its resources change."

- [ ] **Step 5: Run everything**

Run: `cargo test -p ultros-app --lib`
Expected: PASS (all, including `sell_history_key_reads_outliers_not_resource_state`).

Run: `cargo fmt --all && cargo clippy -p ultros-app --all-targets -- -D warnings`
Expected: clean, no dead code anywhere in `analyzer_kit`.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "refactor(recipe-analyzer): fold sell history into one resource; hoist ?sort= to the page"
```

---

### Task 8: Debug timing, full CI check, manual parity, PR

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (the `priced` memo)

- [ ] **Step 1: Add the debug-only timing around `price_rows`**

Inside the `priced` memo, wrap the call:

```rust
            #[cfg(all(debug_assertions, feature = "hydrate"))]
            let t0 = js_sys::Date::now();
            let rows = price_rows(&inp);
            #[cfg(all(debug_assertions, feature = "hydrate"))]
            leptos::logging::log!(
                "price_rows: {} recipes priced in {:.1} ms",
                rows.len(),
                js_sys::Date::now() - t0
            );
            Arc::new(rows.into_iter().map(Arc::new).collect())
```

`js-sys` is already an optional dependency of `ultros-app` (Cargo.toml:76); confirm it is enabled by the `hydrate` feature (grep `hydrate = [` in `ultros-frontend/ultros-app/Cargo.toml`); if it is not, add `"dep:js-sys"` to that feature list.

- [ ] **Step 2: Full CI check**

Run from the repo root:

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`.

- [ ] **Step 3: Manual parity against `main`**

Build and serve (`cargo leptos serve`, or `./scripts/run_e2e.sh` with `E2E_PORT` pinned) and open `/recipe-analyzer?world=Gilgamesh` on this branch and on `main` side by side. Check: the same row set and the same order under the default sort, except among rows tied on the sort key, where this branch orders by recipe key id while `main` follows `HashMap` order (tie-heavy sorts such as `?sort=confidence`, `?sort=last-sold` and `?sort=tax` differ inside tie groups only); the same rows after `?sort=cost`, `?sort=roi&dir=asc`, `?cost-basis=sale-median`, `?buy-scope=world&cost-basis=sale-median`, `?filter-outliers=true`; the network tab shows the same requests as `main` on every one of those URLs (default: cheapest ×2, sale_stats ×1). Read the `price_rows:` log line in the console and record the number in the PR description.

- [ ] **Step 4: Commit and open the PR**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-app/Cargo.toml
git commit -m "chore(recipe-analyzer): debug timing around price_rows"
git push -u origin HEAD
gh pr create --base main --title "Analyzer kit phase A: formula core, zero-copy pricing, memo split" --body "Part of #1233. Pure refactor: no user-visible change (oracle-pinned). See docs/superpowers/plans/2026-09-01-analyzer-kit-phase-a-formula-core.md.

- analyzer_kit::{formula, signals, needed}: PriceSignal (CostBasis/RevenueMetric unified), ProfitFormula + profit_line, PriceLookup + SignalView (no more map clones), needed_bodies
- compute_cost generic over PriceLookup; callers unchanged
- recipe analyzer: price_rows + filter_and_sort split (sort never re-prices), one sell-history resource (fixes the #1248 resource-in-memo warning), ?sort= hoisted to the page
- the one deliberate ordering change: rows tied on the sort key are ordered by recipe key id (main follows HashMap order there); every number, row set and request is otherwise identical
- price_rows timing on the default view: <fill in> ms for <n> recipes

Tests: cargo test -p ultros-app --lib (all green), ./check_ci.sh clean. Manual parity vs main recorded above."
```

---

## Self-review

**Spec coverage (kit spec Phase A):** formula/signals/layers/needed modules → Tasks 1, 2, 3, 5 (`layers.rs` is deferred to Phase B because nothing in A constructs `Layer`); generic `compute_cost` → Task 4; `PriceSummary::chosen` and `IngredientLine.world_id` → deferred to Phase D, their first consumer (dead code otherwise); map clones → views, indexes hoisted, memo split, `job_filter` read once, on-hand clone under the toggle, `key_id` tiebreak → Task 6; `?sort=` hoisted, folded sell-history resource keyed on URL state, stale comments → Task 7; debug timing → Task 8; raw-sales map stays item-keyed → Task 6 (`raw_sales`); oracle → Task 6 Step 5-6; no changelog → Global Constraints.

**Placeholder scan:** the `todo!` in Task 6 Step 1 is explicitly replaced by the full body shown in the same step; the `ORACLE` constant is filled in Step 6 by a recorded run (the plan gives the command and the exact paste target).

**Type consistency:** `PriceInputs.on_hand: Option<&HashMap<i32, i32>>` matches `LocalOnHand::from_map(HashMap<i32, i32>)`; `StatsIndex` = `HashMap<(i32, bool), ItemSaleStats>` matches `sales_stats_from_rollup`'s parameter; `profit_line` returns `(ProfitLine, bool)` in Tasks 2 and 6; `needed_bodies(&ProfitFormula, &RecipeNeeds)` in Tasks 5 and 7; `SellHistory` fields used in Task 7's join match its definition.
