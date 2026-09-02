//! The profit ledger: which signal feeds each side of
//! `profit = revenue − tax − cost`, and the per-tool policies that make
//! every analyzer's numbers reproducible from one function.

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
        assert_eq!(
            net_after_tax(1_999_999_999, TaxMath::IntegerFloor),
            1_899_999_999
        );
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
}
