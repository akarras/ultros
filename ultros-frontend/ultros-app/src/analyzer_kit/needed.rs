//! Which bulk bodies a view needs, and which cost signals the pricing pass
//! must run per recipe. The page consults the body set for the buy-scope
//! stats gate and hands the signal set to `price_rows`.

use std::collections::BTreeSet;

use super::formula::{BuyScope, PriceSignal, ProfitFormula};

/// The one sale-history window every recipe-analyzer body uses. The
/// server serves 1 | 7 | 30 | 90; the labels in seven locales say "(7d)".
pub const SALE_STATS_WINDOW_DAYS: u16 = 7;

/// The second window, read only by the opt-in 30-day columns. Its body is
/// client-only: it never joins the Suspense gate, so a page that wants it
/// still renders its table first and fills those two cells in after
/// (`LateStats`).
pub const STATS_30_WINDOW_DAYS: u16 = 30;

/// The windows `/api/v1/sale_stats` accepts (`SUPPORTED_WINDOWS` in
/// `ultros/src/web/api/sale_stats.rs`); anything else is a 400.
///
/// Both window constants above are checked against this at compile time.
/// Without that, editing either one to an unsupported value — 14, say —
/// compiles, passes every test in this file, and fails only as a runtime
/// 400 on a deployed page, because nothing on the client side validates
/// the window it asks for.
const SERVER_SUPPORTED_WINDOWS: [u16; 4] = [1, 7, 30, 90];

const fn is_supported_window(days: u16) -> bool {
    let mut i = 0;
    while i < SERVER_SUPPORTED_WINDOWS.len() {
        if SERVER_SUPPORTED_WINDOWS[i] == days {
            return true;
        }
        i += 1;
    }
    false
}

const _: () = assert!(
    is_supported_window(SALE_STATS_WINDOW_DAYS),
    "SALE_STATS_WINDOW_DAYS is not a window the server accepts"
);
const _: () = assert!(
    is_supported_window(STATS_30_WINDOW_DAYS),
    "STATS_30_WINDOW_DAYS is not a window the server accepts"
);

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
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RecipeNeeds {
    /// The opt-in outlier filter reads raw recent sales.
    pub outliers: bool,
    /// Buy from = This world only, and it resolved to the sell world: the
    /// sell-world stats body doubles as the buy-scope body.
    pub buy_scope_is_sell_world: bool,
    /// Every cost signal the pass will run ([`NeededSignals::cost`]); a
    /// visible or sorted sale-cost column needs the buy-scope body even
    /// when the selected signal is the listing.
    pub cost_signals: BTreeSet<PriceSignal>,
    /// A 30-day column (Volume 30d, VWAP 30d) is visible or the sort
    /// target. Not "the lab is on": the body costs 438 KB on the wire, so
    /// only actually asking for one of those columns fetches it.
    pub stats_30: bool,
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
    let wants_sale_stats = formula.cost_signal().sale_stat().is_some()
        || needs.cost_signals.iter().any(|s| s.sale_stat().is_some());
    if wants_sale_stats && !aliased {
        set.insert(BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS));
    }
    if needs.outliers {
        set.insert(BodyRole::RecentSalesSellWorld);
    }
    if needs.stats_30 {
        set.insert(BodyRole::SellWorldStats(STATS_30_WINDOW_DAYS));
    }
    set
}

/// What the visible columns and the sort target ask of the pricing pass,
/// before the sub-craft cap. `visible_cost` is in table order.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SignalWants {
    pub visible_cost: Vec<PriceSignal>,
    pub sort_cost: Option<PriceSignal>,
    pub hop: bool,
    pub worlds: bool,
}

/// The cost signals `price_rows` runs per recipe, plus the two hop flags.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct NeededSignals {
    pub cost: BTreeSet<PriceSignal>,
    /// Requested but not run: the sub-craft cap. Their cells render "—".
    pub capped: BTreeSet<PriceSignal>,
    pub hop: bool,
    pub worlds: bool,
}

/// {effective cost} ∪ {ListingMin when Worlds is wanted} ∪ {the sort
/// target} ∪ {visible cost-* columns}. With sub-crafts on, at most two
/// signals beyond the selected one are kept, claimed in that order; the
/// rest are `capped`. Once the cap is full, every `PriceSignal` not in the
/// result — including ones the caller never asked for — is also marked
/// `capped`, so the picker can grey the entries a player has not ticked yet
/// rather than let them tick a column that renders "—". Enforced here, not
/// in the picker, so it holds for any bookmarked URL and identically on SSR
/// and CSR.
pub fn needed_signals(
    formula: &ProfitFormula,
    wants: &SignalWants,
    use_subcrafts: bool,
) -> NeededSignals {
    let selected = formula.cost_signal();
    let cap = if use_subcrafts { 2 } else { usize::MAX };
    let mut cost = BTreeSet::from([selected]);
    let mut capped = BTreeSet::new();
    let mut extras = 0usize;
    let mut claim =
        |s: PriceSignal, cost: &mut BTreeSet<PriceSignal>, capped: &mut BTreeSet<PriceSignal>| {
            if cost.contains(&s) {
                return;
            }
            if extras < cap {
                cost.insert(s);
                extras += 1;
            } else {
                capped.insert(s);
            }
        };
    if wants.worlds {
        claim(PriceSignal::ListingMin, &mut cost, &mut capped);
    }
    if let Some(s) = wants.sort_cost {
        claim(s, &mut cost, &mut capped);
    }
    for s in &wants.visible_cost {
        claim(*s, &mut cost, &mut capped);
    }
    // The cap is full: mark every OTHER cost signal capped too, requested
    // or not, so the picker greys entries a player has not ticked yet
    // instead of letting them tick a fourth column that only then shows
    // the hint.
    if use_subcrafts && extras == cap {
        for s in PriceSignal::ALL {
            if !cost.contains(&s) {
                capped.insert(s);
            }
        }
    }
    NeededSignals {
        cost,
        capped,
        hop: wants.hop,
        worlds: wants.worlds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer_kit::formula::{BuyScope, PriceSignal, ProfitFormula};

    fn needs(outliers: bool, same: bool) -> RecipeNeeds {
        RecipeNeeds {
            outliers,
            buy_scope_is_sell_world: same,
            cost_signals: BTreeSet::new(),
            stats_30: false,
        }
    }

    fn set(signals: &[PriceSignal]) -> BTreeSet<PriceSignal> {
        signals.iter().copied().collect()
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

    #[test]
    fn thirty_day_columns_need_a_second_sell_world_body() {
        let f = ProfitFormula::recipe_from_query(None, None, None);
        let base = needed_bodies(&f, &needs(false, false));
        assert!(!base.contains(&BodyRole::SellWorldStats(STATS_30_WINDOW_DAYS)));
        let wants = RecipeNeeds {
            stats_30: true,
            ..needs(false, false)
        };
        let got = needed_bodies(&f, &wants);
        assert!(got.contains(&BodyRole::SellWorldStats(STATS_30_WINDOW_DAYS)));
        // Two windows are two bodies: the 7-day one is still needed.
        assert!(got.contains(&BodyRole::SellWorldStats(SALE_STATS_WINDOW_DAYS)));
        assert_eq!(got.len(), base.len() + 1);
    }

    #[test]
    fn needed_signals_is_selection_union_visible_union_sort_target() {
        let f = ProfitFormula::recipe_from_query(Some(PriceSignal::SaleMedian), None, None);
        let wants = SignalWants {
            visible_cost: vec![PriceSignal::ListingMin, PriceSignal::SaleMedian],
            sort_cost: Some(PriceSignal::SaleAvg),
            hop: false,
            worlds: true,
        };
        let got = needed_signals(&f, &wants, false);
        assert_eq!(
            got.cost,
            set(&[
                PriceSignal::ListingMin,
                PriceSignal::SaleMedian,
                PriceSignal::SaleAvg
            ])
        );
        assert!(got.capped.is_empty());
        assert!(!got.hop);
        assert!(got.worlds);
        // The default: exactly the selected signal, nothing else.
        let plain = needed_signals(&f, &SignalWants::default(), false);
        assert_eq!(plain.cost, set(&[PriceSignal::SaleMedian]));
        assert!(!plain.hop && !plain.worlds);
    }

    #[test]
    fn needed_signals_sets_hop_when_a_hop_column_is_the_sort_target() {
        let f = ProfitFormula::recipe_from_query(None, None, None);
        let wants = SignalWants {
            hop: true,
            ..SignalWants::default()
        };
        assert!(needed_signals(&f, &wants, false).hop);
        let wants = SignalWants {
            worlds: true,
            ..SignalWants::default()
        };
        let got = needed_signals(&f, &wants, false);
        assert!(got.worlds);
        assert!(
            got.cost.contains(&PriceSignal::ListingMin),
            "Worlds needs the listing-min run"
        );
    }

    /// The cap lives here, not in the picker, so a bookmarked `?cols=` with
    /// four cost columns and sub-crafts on prices the selected signal plus
    /// two extras and marks the rest capped; identically on SSR and CSR.
    #[test]
    fn subcraft_cap_applies_to_url_bookmarks() {
        let f = ProfitFormula::recipe_from_query(None, None, None); // listing-min
        let all = vec![
            PriceSignal::ListingMin,
            PriceSignal::SaleMin,
            PriceSignal::SaleMedian,
            PriceSignal::SaleAvg,
        ];
        let wants = SignalWants {
            visible_cost: all.clone(),
            ..SignalWants::default()
        };
        let got = needed_signals(&f, &wants, true);
        assert_eq!(
            got.cost,
            set(&[
                PriceSignal::ListingMin,
                PriceSignal::SaleMin,
                PriceSignal::SaleMedian
            ])
        );
        assert_eq!(got.capped, set(&[PriceSignal::SaleAvg]));
        // Without sub-crafts nothing is capped.
        let got = needed_signals(&f, &wants, false);
        assert_eq!(got.cost.len(), 4);
        assert!(got.capped.is_empty());
        // The sort target and Worlds' listing-min take slots before visible columns.
        let f = ProfitFormula::recipe_from_query(Some(PriceSignal::SaleMedian), None, None);
        let wants = SignalWants {
            visible_cost: all,
            sort_cost: Some(PriceSignal::SaleAvg),
            hop: false,
            worlds: true,
        };
        let got = needed_signals(&f, &wants, true);
        assert_eq!(
            got.cost,
            set(&[
                PriceSignal::SaleMedian,
                PriceSignal::ListingMin,
                PriceSignal::SaleAvg
            ])
        );
        assert_eq!(got.capped, set(&[PriceSignal::SaleMin]));
    }

    /// Once the cap is full every unrequested cost signal is capped too, so
    /// the picker can grey the entries a player has not ticked yet.
    #[test]
    fn full_cap_greys_the_unrequested_signals() {
        let f = ProfitFormula::recipe_from_query(None, None, None); // listing-min
        let wants = SignalWants {
            visible_cost: vec![PriceSignal::SaleMin, PriceSignal::SaleMedian],
            ..SignalWants::default()
        };
        let got = needed_signals(&f, &wants, true);
        assert_eq!(
            got.cost,
            set(&[
                PriceSignal::ListingMin,
                PriceSignal::SaleMin,
                PriceSignal::SaleMedian
            ])
        );
        assert_eq!(
            got.capped,
            set(&[PriceSignal::SaleAvg]),
            "the untouched fourth signal is capped"
        );
        // One extra short of the cap: nothing is capped.
        let wants = SignalWants {
            visible_cost: vec![PriceSignal::SaleMin],
            ..SignalWants::default()
        };
        assert!(needed_signals(&f, &wants, true).capped.is_empty());
        // Sub-crafts off: never capped.
        let wants = SignalWants {
            visible_cost: PriceSignal::ALL.to_vec(),
            ..SignalWants::default()
        };
        assert!(needed_signals(&f, &wants, false).capped.is_empty());
    }

    #[test]
    fn visible_sale_cost_column_needs_the_buy_scope_body() {
        let f = ProfitFormula::recipe_from_query(None, None, None); // listing-min selected
        let mut n = needs(false, false);
        n.cost_signals = set(&[PriceSignal::ListingMin, PriceSignal::SaleMin]);
        assert!(needed_bodies(&f, &n).contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS)));
        n.cost_signals = set(&[PriceSignal::ListingMin]);
        assert!(!needed_bodies(&f, &n).contains(&BodyRole::BuyScopeStats(SALE_STATS_WINDOW_DAYS)));
    }
}
