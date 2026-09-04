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

    /// Every signal in token order; also the index order of the per-signal
    /// arrays a priced row carries (`cost_alt`, `rev_alt`).
    pub const ALL: [PriceSignal; 4] = [
        PriceSignal::ListingMin,
        PriceSignal::SaleMin,
        PriceSignal::SaleMedian,
        PriceSignal::SaleAvg,
    ];

    /// Position in [`PriceSignal::ALL`].
    pub fn index(self) -> usize {
        match self {
            PriceSignal::ListingMin => 0,
            PriceSignal::SaleMin => 1,
            PriceSignal::SaleMedian => 2,
            PriceSignal::SaleAvg => 3,
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

/// The same three places, named for what they are when they are not the
/// buy side's: the sell world, its datacenter, or the whole region. The
/// spec calls the shared enum `Scope`; `BuyScope` keeps its name at the
/// ~60 sites that already spell it.
pub type Scope = BuyScope;

/// Where the *product's price is read* — [`ProfitFormula::sell_scope`]'s
/// URL value under `?sell-scope=`.
///
/// Named for the sale, not for a destination: FFXIV retainers list only on
/// their own world, so a wider sell scope is a reference read ("what does
/// this go for across my datacenter"), never somewhere to go and sell.
///
/// A newtype over [`Scope`] rather than a bare `Scope`, because
/// `Scope::default()` is `Datacenter`: that is the **buy** side's default,
/// and the sell side's is the world. A bare `param.unwrap_or_default()`, or
/// the default-stripping setter idiom this repo writes everywhere
/// (`parsed.filter(|s| *s != Scope::default())`), would silently re-price
/// every existing recipe-analyzer URL across the datacenter and strip the
/// wrong token out of the URL. Both idioms are correct on this type.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SellScope(pub Scope);

impl Default for SellScope {
    fn default() -> Self {
        SellScope(Scope::World)
    }
}

impl SellScope {
    pub fn scope(self) -> Scope {
        self.0
    }
}

impl FromStr for SellScope {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<Scope>().map(SellScope)
    }
}

impl Display for SellScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
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

/// How the 5% is rounded.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TaxMath {
    /// `net = gross * 95 / 100` in integer math: the *net* is floored, so
    /// the tax itself rounds up (5% of 3,911 shows as 196, not 195). The
    /// flip finder and vendor pages truncate an f32 instead; the two agree
    /// below 2,207,541 gil.
    IntegerFloor,
}

/// How ROI is computed. The recipe analyzer's unclamped f64 division is
/// kept here so Phase A changes no number; Phase C adopts the clamp.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RoiMath {
    UnclampedF64,
    /// `analysis::return_on_investment`: f64 ratio, clamped at ±100,000
    /// and truncated to i32.
    ClampedF64,
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
    /// Where the product's price is read. `Fixed(Scope::World)` — today's
    /// and every pre-Phase-F URL's value — until
    /// [`ProfitFormula::with_sell_scope`] seats it, which only the recipe
    /// analyzer does and only under the `analyzer-recipe` lab.
    pub sell_scope: Term<Scope>,
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

    /// Seat the sell side's scope. Phase F's one entry point: a caller that
    /// never calls this keeps `Term::Fixed(Scope::World)`, which is
    /// `PartialEq`-identical to what `recipe_from_query` has always
    /// produced, so the flag-off page's `Memo<ProfitFormula>` cannot fire
    /// on it. Takes a [`SellScope`], never an `Option<Scope>`: see the
    /// newtype's doc for why the default matters.
    ///
    /// Exactly one caller in the crate — `recipe_analyzer::seat_sell_scope`.
    /// The page and the table build their formulas in two different places
    /// and only the table's prices rows, so the seating goes through one
    /// function that both of them (and the pricing test harness) call.
    pub fn with_sell_scope(mut self, sell: SellScope) -> Self {
        self.sell_scope = Term::Select(sell.scope());
        self
    }

    /// Where revenue is priced: the sell world (the default), its
    /// datacenter, or the region.
    pub fn sell_scope(&self) -> Scope {
        self.sell_scope.value()
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

/// What the header marks and the readout need to know about the
/// selected formula, with places already resolved to names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaMarks {
    pub revenue: PriceSignal,
    pub cost: PriceSignal,
    pub sell_place: String,
    pub buy_place: String,
}

impl ProfitFormula {
    pub fn marks(&self, sell_place: String, buy_place: String) -> FormulaMarks {
        FormulaMarks {
            revenue: self.revenue_signal(),
            cost: self.cost_signal(),
            sell_place,
            buy_place,
        }
    }
}

/// One row's arithmetic under the selected formula.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProfitLine {
    pub revenue: i32,
    pub tax: i32,
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
        RoiMath::ClampedF64 => crate::analysis::return_on_investment(profit, cost_per_unit),
    };
    let dropped = match f.drop {
        DropRule::CostAtOrAboveNet => cost_per_unit >= net,
    };
    (
        ProfitLine {
            revenue: gross,
            tax,
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

    #[test]
    fn price_signal_index_matches_all_order() {
        for (i, s) in PriceSignal::ALL.iter().enumerate() {
            assert_eq!(s.index(), i);
        }
        assert_eq!(PriceSignal::ALL[0], PriceSignal::ListingMin);
        assert_eq!(PriceSignal::ALL[3], PriceSignal::SaleAvg);
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
        assert_eq!(line.revenue - line.tax, 11_932);
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
        assert_eq!(line.revenue - line.tax, 949_999);
        assert_eq!(line.profit, 949_738);
        assert_eq!(line.roi, 363_884);
        // Cost 0 → ROI 0, never a division by zero.
        let (line, dropped) = profit_line(100, 0, &f);
        assert!(!dropped);
        assert_eq!(line.roi, 0);
    }

    #[test]
    fn roi_is_clamped_at_display_ceiling_when_asked() {
        let mut f = recipe_default();
        f.roi = RoiMath::ClampedF64;
        let (line, _) = profit_line(999_999, 261, &f);
        assert_eq!(line.roi, 100_000);
        let (line, _) = profit_line(12_560, 11_300, &f);
        assert_eq!(line.roi, 5);
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

    /// The sell side's default is the sell WORLD. `Scope::default()` is
    /// `Datacenter` — the buy side's default — so a bare
    /// `unwrap_or_default()` here, or a `filter(|s| *s != Scope::default())`
    /// default-stripping setter, would move every existing URL's revenue to
    /// the datacenter. The newtype is what makes both idioms correct.
    #[test]
    fn sell_scope_defaults_to_the_world_not_the_buy_sides_datacenter() {
        assert_eq!(SellScope::default().scope(), Scope::World);
        assert_ne!(SellScope::default().scope(), Scope::default());
        assert_eq!(Scope::default(), Scope::Datacenter);
    }

    #[test]
    fn sell_scope_tokens_are_the_buy_scope_tokens() {
        for s in ["world", "datacenter", "region"] {
            assert_eq!(s.parse::<SellScope>().unwrap().to_string(), s);
        }
        assert_eq!(SellScope::default().to_string(), "world");
        assert!("home".parse::<SellScope>().is_err());
    }

    /// A formula that never seats the sell scope is byte-identical to
    /// today's: `Fixed(World)`, not `Select(World)`. That is what keeps the
    /// flag-off `Memo<ProfitFormula>` from firing on a value nothing reads.
    #[test]
    fn with_sell_scope_is_the_only_way_to_move_the_sell_side() {
        let untouched = ProfitFormula::recipe_from_query(None, None, None);
        assert_eq!(untouched.sell_scope, Term::Fixed(Scope::World));
        assert_eq!(untouched.sell_scope(), Scope::World);

        let seated = untouched.with_sell_scope(SellScope::default());
        assert_eq!(seated.sell_scope, Term::Select(Scope::World));
        assert_eq!(seated.sell_scope(), Scope::World);

        let region = untouched.with_sell_scope(SellScope(Scope::Region));
        assert_eq!(region.sell_scope(), Scope::Region);
        // Nothing else in the ledger moved.
        assert_eq!(region.cost_signal(), untouched.cost_signal());
        assert_eq!(region.revenue_signal(), untouched.revenue_signal());
        assert_eq!(region.buy_scope(), untouched.buy_scope());
        assert_eq!(region.tax, untouched.tax);
        assert_eq!(region.roi, untouched.roi);
        assert_eq!(region.drop, untouched.drop);
    }
}
