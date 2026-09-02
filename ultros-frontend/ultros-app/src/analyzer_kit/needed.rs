//! Which bulk bodies a view needs. Today the page consults the set only
//! for the buy-scope stats gate; later phases wire the remaining roles.

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer_kit::formula::{BuyScope, PriceSignal, ProfitFormula};

    fn needs(outliers: bool, same: bool) -> RecipeNeeds {
        RecipeNeeds {
            outliers,
            buy_scope_is_sell_world: same,
        }
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
