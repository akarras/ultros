//! Bulk basket: the cheapest set of whole listings that fills a quantity of a
//! single item, per world and across worlds.
//!
//! Market-board listings cannot be split, so walking listings cheapest-first
//! is not the answer; this reuses the planner's exact stack knapsack
//! ([`purchase_with_budget`]) and only adds the per-world grouping and the
//! "is spreading the order worth showing" decision.

use crate::recipe_planner::{Offer, Purchase, purchase_with_budget};
use std::collections::{BTreeMap, BTreeSet};
use ultros_api_types::ActiveListing;

/// Knapsack cells solved exactly. The basket solves each world once per
/// listings update (not once per route like the planner), so it can afford a
/// region-wide 999-unit order before falling back to the greedy estimate.
pub const BASKET_BUDGET: usize = 1_000_000;

/// The listings one basket buys, with the counts the item page shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Basket {
    pub purchase: Purchase,
    /// Worlds the chosen listings sit on.
    pub worlds: BTreeSet<i32>,
    /// Number of listings bought.
    pub listings: usize,
    /// Distinct retainers bought from.
    pub retainers: usize,
}

impl Basket {
    fn new(purchase: Purchase, retainer_of: &BTreeMap<i64, i32>) -> Self {
        Basket {
            worlds: purchase.offers.iter().map(|o| o.world).collect(),
            listings: purchase.offers.len(),
            retainers: purchase
                .offers
                .iter()
                .filter_map(|o| retainer_of.get(&o.id))
                .collect::<BTreeSet<_>>()
                .len(),
            purchase,
        }
    }

    pub fn cost(&self) -> i64 {
        self.purchase.cost
    }

    /// Units received. Whole stacks can overshoot the order.
    pub fn received(&self) -> i64 {
        self.purchase.quantity
    }

    pub fn missing(&self) -> i64 {
        self.purchase.missing()
    }

    pub fn complete(&self) -> bool {
        self.missing() == 0
    }

    /// True when the solver fell back to its greedy best-found estimate.
    pub fn approximate(&self) -> bool {
        self.purchase.approximate
    }

    /// Gil per unit received, rounded to the nearest gil; `None` when the
    /// basket is empty.
    pub fn unit_price(&self) -> Option<i64> {
        let received = self.received();
        (received > 0).then(|| (2 * self.cost() + received) / (2 * received))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldBasket {
    pub world: i32,
    pub basket: Basket,
}

/// The order spread over several worlds, shown only when that buys something
/// no single world does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrossWorldBasket {
    pub basket: Basket,
    /// `(world, saving)` against the best single world, when that world can
    /// fill the order on its own. `None` means no single world can, and the
    /// spread basket is shown because it fills more.
    pub versus: Option<(i32, i64)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BulkBaskets {
    /// One row per world with listings: complete worlds cheapest first, then
    /// short worlds by smallest shortfall.
    pub worlds: Vec<WorldBasket>,
    pub cross_world: Option<CrossWorldBasket>,
}

/// Cheapest whole listings filling `needed` units from `listings`, per world
/// and across worlds. Callers filter quality and excluded worlds first.
pub fn bulk_baskets<'a>(
    needed: i64,
    listings: impl IntoIterator<Item = &'a ActiveListing>,
) -> BulkBaskets {
    if needed <= 0 {
        return BulkBaskets::default();
    }
    let mut retainer_of = BTreeMap::new();
    let mut by_world: BTreeMap<i32, Vec<Offer>> = BTreeMap::new();
    for listing in listings {
        retainer_of.insert(listing.id, listing.retainer_id);
        by_world.entry(listing.world_id).or_default().push(Offer {
            id: listing.id,
            world: listing.world_id,
            quantity: listing.quantity.into(),
            price: listing.price_per_unit.into(),
        });
    }
    let solve = |offers: &[Offer]| {
        Basket::new(
            purchase_with_budget(needed, offers, None, BASKET_BUDGET),
            &retainer_of,
        )
    };

    let mut worlds: Vec<_> = by_world
        .iter()
        .map(|(world, offers)| WorldBasket {
            world: *world,
            basket: solve(offers),
        })
        .filter(|w| w.basket.listings > 0)
        .collect();
    worlds.sort_by_key(|w| (w.basket.missing(), w.basket.cost(), w.world));

    let cross_world = match worlds.first() {
        Some(best) if worlds.len() > 1 => {
            let all: Vec<_> = by_world.values().flatten().cloned().collect();
            let basket = solve(&all);
            let versus = best
                .basket
                .complete()
                .then(|| (best.world, best.basket.cost() - basket.cost()));
            // Worth a row only if it beats every single world: cheaper when
            // one world can already fill the order, fuller when none can. An
            // approximate spread can lose to an exact world; hide it then.
            let better = match versus {
                Some((_, saving)) => basket.complete() && saving > 0,
                None => basket.missing() < best.basket.missing(),
            };
            (better && basket.worlds.len() > 1).then_some(CrossWorldBasket { basket, versus })
        }
        _ => None,
    };

    BulkBaskets {
        worlds,
        cross_world,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe_planner::purchase;

    fn listing(id: i32, world: i32, retainer: i32, quantity: i32, price: i32) -> ActiveListing {
        ActiveListing {
            id: id.into(),
            world_id: world,
            item_id: 5057,
            retainer_id: retainer,
            price_per_unit: price,
            quantity,
            hq: false,
            timestamp: Default::default(),
        }
    }

    fn world_order(baskets: &BulkBaskets) -> Vec<i32> {
        baskets.worlds.iter().map(|w| w.world).collect()
    }

    #[test]
    fn whole_stacks_beat_the_cheapest_first_walk() {
        // Walking by unit price buys 50@10 then the whole 99@11 stack: 149
        // units for 1,589. Skipping to the dearer 49-stack fills exactly for
        // 1,088, a gil under buying the 99-stack alone.
        let listings = [
            listing(1, 10, 1, 50, 10),
            listing(2, 10, 2, 99, 11),
            listing(3, 10, 3, 49, 12),
        ];
        let baskets = bulk_baskets(99, &listings);
        let basket = &baskets.worlds[0].basket;
        assert_eq!(basket.cost(), 50 * 10 + 49 * 12);
        assert_eq!(basket.received(), 99);
        assert_eq!(basket.listings, 2);
        assert!(basket.complete());
        assert!(!basket.approximate());
    }

    #[test]
    fn a_short_world_buys_everything_and_reports_the_gap() {
        let listings = [listing(1, 10, 1, 25, 10), listing(2, 10, 2, 15, 20)];
        let basket = bulk_baskets(99, &listings).worlds.remove(0).basket;
        assert_eq!(basket.received(), 40);
        assert_eq!(basket.missing(), 59);
        assert_eq!(basket.cost(), 25 * 10 + 15 * 20);
        assert!(!basket.complete());
    }

    #[test]
    fn a_whole_stack_can_overshoot_the_order() {
        let listings = [listing(1, 10, 1, 105, 10)];
        let basket = bulk_baskets(99, &listings).worlds.remove(0).basket;
        assert_eq!(basket.received(), 105);
        assert_eq!(basket.missing(), 0);
        assert_eq!(basket.cost(), 1_050);
        assert_eq!(basket.unit_price(), Some(10));
    }

    #[test]
    fn retainers_are_counted_once_per_retainer() {
        let listings = [
            listing(1, 10, 7, 10, 10),
            listing(2, 10, 7, 10, 11),
            listing(3, 10, 8, 10, 12),
        ];
        let basket = bulk_baskets(30, &listings).worlds.remove(0).basket;
        assert_eq!(basket.listings, 3);
        assert_eq!(basket.retainers, 2);
        assert_eq!(basket.worlds, BTreeSet::from([10]));
    }

    #[test]
    fn complete_worlds_come_first_cheapest_then_smallest_shortfall() {
        let listings = [
            listing(1, 1, 1, 10, 30), // complete, 300
            listing(2, 2, 2, 10, 20), // complete, 200
            listing(3, 3, 3, 1, 5),   // short 9
            listing(4, 4, 4, 5, 5),   // short 5
        ];
        let baskets = bulk_baskets(10, &listings);
        assert_eq!(world_order(&baskets), vec![2, 1, 4, 3]);
    }

    #[test]
    fn spreading_across_worlds_is_shown_with_its_saving() {
        let listings = [
            listing(1, 1, 1, 50, 10),
            listing(2, 1, 2, 50, 30),
            listing(3, 2, 3, 50, 10),
            listing(4, 2, 4, 50, 30),
        ];
        let baskets = bulk_baskets(100, &listings);
        let cross = baskets.cross_world.expect("spreading halves the bill");
        assert_eq!(cross.basket.cost(), 1_000);
        assert_eq!(cross.basket.worlds, BTreeSet::from([1, 2]));
        // Worlds 1 and 2 tie at 2,000; the lower id is the reference.
        assert_eq!(cross.versus, Some((1, 1_000)));
    }

    #[test]
    fn no_spread_row_when_one_world_is_already_cheapest() {
        let listings = [listing(1, 1, 1, 100, 10), listing(2, 2, 2, 100, 20)];
        assert_eq!(bulk_baskets(100, &listings).cross_world, None);
    }

    #[test]
    fn no_spread_row_for_a_single_world_scope() {
        let listings = [listing(1, 1, 1, 50, 10), listing(2, 1, 2, 50, 30)];
        assert_eq!(bulk_baskets(100, &listings).cross_world, None);
    }

    #[test]
    fn spreading_fills_an_order_no_single_world_can() {
        let listings = [listing(1, 1, 1, 60, 10), listing(2, 2, 2, 60, 10)];
        let baskets = bulk_baskets(100, &listings);
        assert!(baskets.worlds.iter().all(|w| !w.basket.complete()));
        let cross = baskets.cross_world.expect("two worlds cover the order");
        assert!(cross.basket.complete());
        assert_eq!(cross.basket.received(), 120);
        assert_eq!(cross.versus, None);
    }

    #[test]
    fn a_region_sized_order_stays_exact_under_the_basket_budget() {
        // 999 units against 500 listings is 499,500 knapsack cells: past the
        // planner's budget, inside the basket's.
        let listings: Vec<_> = (0..500)
            .map(|i| listing(i, 1 + i % 5, i, 3, 100 + i % 37))
            .collect();
        let offers: Vec<_> = listings
            .iter()
            .map(|l| Offer {
                id: l.id,
                world: l.world_id,
                quantity: l.quantity as i64,
                price: l.price_per_unit as i64,
            })
            .collect();
        assert!(purchase(999, &offers, None).approximate);
        let cross = bulk_baskets(999, &listings)
            .cross_world
            .expect("no single world holds 999 units");
        assert!(!cross.basket.approximate());
        assert!(cross.basket.complete());
    }

    #[test]
    fn unit_price_rounds_to_the_nearest_gil() {
        let listings = [listing(1, 1, 1, 2, 500), listing(2, 1, 2, 1, 1)];
        // 1,001 gil for 3 units = 333.67
        let basket = bulk_baskets(3, &listings).worlds.remove(0).basket;
        assert_eq!(basket.unit_price(), Some(334));
        assert_eq!(Basket::default().unit_price(), None);
    }

    #[test]
    fn nothing_to_buy_is_empty() {
        let listings = [listing(1, 1, 1, 10, 10)];
        assert_eq!(bulk_baskets(0, &listings), BulkBaskets::default());
        assert_eq!(bulk_baskets(10, &[]), BulkBaskets::default());
    }
}
