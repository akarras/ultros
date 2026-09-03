//! Crafting cost types and computation — scaffolded in Task 1, implemented in Tasks 2-4.
// Dead-code allow retained for `item_page_default` — a test-only convenience
// factory kept around for future surfaces that want a one-call defaults builder
// rather than the inline-literal pattern the analyzers/item page currently use.
// Drop the allow (and the helper) if a real caller never materializes.
#![allow(dead_code)]

use crate::analyzer_kit::signals::PriceLookup;
use std::collections::HashMap;
use xiv_gen::{ItemId, Recipe};

/// Crystal/shard/cluster items are item_search_category == 59 in xiv-gen.
/// Matches the convention used in add_recipe_to_current_list.rs.
pub const CRYSTAL_SEARCH_CATEGORY: i32 = 59;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ShardsMode {
    #[default]
    ExcludeShards,
    IncludeMarket,
}

/// Where an ingredient line's `unit_price` came from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PriceSource {
    /// Cheapest matching market listing.
    Market,
    /// NPC gil-shop price (always NQ; never used when `require_hq`).
    Vendor,
    /// Crafting the ingredient via a sub-recipe was cheaper than buying.
    Subcraft,
}

pub struct CraftingCostOptions<'a> {
    pub require_hq: bool,
    pub max_subcraft_depth: u8,
    pub shards: ShardsMode,
    pub on_hand: &'a dyn OnHand,
    /// item_id -> NPC gil-shop unit price. `None` disables the vendor floor.
    pub vendor_prices: Option<&'a HashMap<i32, i32>>,
}

impl<'a> CraftingCostOptions<'a> {
    /// Defaults that match the existing item-page behavior (no subcrafts,
    /// no HQ preference, no on-hand) plus the new ExcludeShards default.
    pub fn item_page_default(on_hand: &'a dyn OnHand) -> Self {
        Self {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand,
            vendor_prices: Some(vendor_price_map()),
        }
    }
}

/// On-hand inventory accounting. `available` returns the qty the user
/// has stockpiled; `consume` is called by `compute_cost` to deduct
/// usage within a single computation pass (prevents the same 100 shards
/// from being credited against two ingredient lines).
pub trait OnHand {
    fn available(&self, item: ItemId) -> i32;
    /// Deduct `qty` units from the on-hand pool for `item`.
    /// Implementations that track state must use interior mutability
    /// (e.g. `RefCell<HashMap<i32, i32>>`) because `compute_cost`
    /// holds a shared reference to `opts.on_hand` across the ingredient walk.
    fn consume(&self, item: ItemId, qty: i32);
}

/// Empty on-hand source — every `available` returns 0. Used by default
/// and as a sentinel where no on-hand panel is visible.
#[derive(Default)]
pub struct EmptyOnHand;

impl OnHand for EmptyOnHand {
    fn available(&self, _item: ItemId) -> i32 {
        0
    }
    fn consume(&self, _item: ItemId, _qty: i32) {}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IngredientLine {
    pub item_id: ItemId,
    pub needed_total: i32,
    pub used_from_on_hand: i32,
    pub used_from_market: i32,
    pub unit_price: i32,
    pub is_shard: bool,
    pub source: PriceSource,
    /// World the chosen market listing sits on; 0 for vendor, sub-craft
    /// and unpriced lines.
    pub world_id: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubcraftInfo {
    pub item_id: ItemId,
    pub amount: i32,
    pub unit_cost: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CostBreakdown {
    /// Resolved cost for the `require_hq` flavor of the caller's options.
    /// Surfaces that need both HQ and LQ totals call `compute_cost` twice
    /// (once with each flavor) and read `.cost` from each result.
    pub cost: i32,
    pub shard_cost: i32,
    pub on_hand_savings: i32,
    pub ingredient_lines: Vec<IngredientLine>,
    pub sub_crafts: Vec<SubcraftInfo>,
    /// Lines bought on a market that no listing priced (`unit_price == 0`),
    /// after the shard flag and the sub-craft pass: shards under
    /// `ExcludeShards` and vendor-sold items are not counted, and the
    /// winning sub-run's count propagates up.
    pub unpriced_market_lines: u16,
}

/// Iterator over the (non-zero) ingredients of a recipe. Moved from
/// related_items.rs unchanged; re-exported there for transition.
#[derive(Copy, Clone, Debug)]
pub struct IngredientsIter<'a>(&'a Recipe, i32);

impl<'a> IngredientsIter<'a> {
    pub fn new(recipe: &'a Recipe) -> Self {
        Self(recipe, 0)
    }
}

impl<'a> Iterator for IngredientsIter<'a> {
    type Item = (ItemId, i32);
    fn next(&mut self) -> Option<Self::Item> {
        while (self.1 as usize) < self.0.ingredient.len() {
            let idx = self.1 as usize;
            let raw_id = self.0.ingredient[idx];
            let amount = self.0.amount_ingredient[idx];
            self.1 += 1;
            if raw_id != 0 {
                return Some((ItemId(raw_id), amount));
            }
        }
        None
    }
}

pub fn compute_ingredient_cost<P: PriceLookup + ?Sized>(
    item_id: ItemId,
    amount_needed: i32,
    prices: &P,
    opts: &CraftingCostOptions<'_>,
) -> IngredientLine {
    // The listing lowest_gil / price_preferring_hq would price from, kept
    // whole so the line can say which world it was priced on.
    let summary = prices.find_matching_listings(item_id.0);
    let chosen = summary.chosen(opts.require_hq);
    let market_price = chosen.map(|c| c.price).unwrap_or(0);

    // Vendor floor: NPC gil-shop goods are always NQ, so never apply when the
    // caller requires HQ. A vendor price of 0 (or missing) is ignored.
    let vendor = if opts.require_hq {
        None
    } else {
        opts.vendor_prices
            .and_then(|m| m.get(&item_id.0).copied())
            .filter(|p| *p > 0)
    };
    let (unit_price, source) = match vendor {
        Some(v) if market_price == 0 || v < market_price => (v, PriceSource::Vendor),
        _ => (market_price, PriceSource::Market),
    };

    // is_shard is set by the recipe-walking caller in Task 3 (which has
    // access to tracked_data().items). The primitive stays pure of
    // game-data lookups so it's trivially testable.
    let is_shard = false;

    // Apply on-hand. The trait may mutate (LocalOnHand uses interior
    // mutability) so we consume eagerly.
    let on_hand_available = opts.on_hand.available(item_id);
    let used_from_on_hand = on_hand_available.min(amount_needed).max(0);
    if used_from_on_hand > 0 {
        opts.on_hand.consume(item_id, used_from_on_hand);
    }
    let used_from_market = (amount_needed - used_from_on_hand).max(0);

    let world_id = match source {
        PriceSource::Market if unit_price > 0 => chosen.map(|c| c.world_id).unwrap_or(0),
        _ => 0,
    };

    IngredientLine {
        item_id,
        needed_total: amount_needed,
        used_from_on_hand,
        used_from_market,
        unit_price,
        is_shard,
        source,
        world_id,
    }
}

use std::sync::OnceLock;

/// item_id -> NPC gil-shop unit price, for every gil-shop-sold item.
/// Built once per process from game data (same construction as the
/// vendor-resale page).
pub fn vendor_price_map() -> &'static HashMap<i32, i32> {
    static MAP: OnceLock<HashMap<i32, i32>> = OnceLock::new();
    MAP.get_or_init(|| {
        let data = crate::global_state::xiv_data::tracked_data();
        let mut map = HashMap::new();
        for items in data.gil_shop_items.values() {
            for shop_item in items {
                if let Some(item) = data.items.get(&xiv_gen::ItemId(shop_item.item))
                    && item.price_mid > 0
                {
                    map.insert(shop_item.item, item.price_mid as i32);
                }
            }
        }
        map
    })
}

#[cfg(test)]
pub mod fixtures;

/// Compute the cost of one execution of `recipe`.
///
/// `is_shard` returns true for ingredient item ids whose `item_search_category == 59`.
/// In production this is `|id| tracked_data().items.get(&id).map(|i| i.item_search_category == 59).unwrap_or(false)`.
/// In tests this is a closure over a fixture HashMap.
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
    let mut cost: i64 = 0;
    let mut shard_cost: i64 = 0;
    let mut on_hand_savings: i64 = 0;
    let mut ingredient_lines: Vec<IngredientLine> = Vec::new();
    let mut sub_crafts: Vec<SubcraftInfo> = Vec::new();
    let mut unpriced: u16 = 0;

    for (item_id, amount) in IngredientsIter::new(recipe) {
        let mut line = compute_ingredient_cost(item_id, amount, prices, opts);
        line.is_shard = is_shard(item_id);

        // Subcraft check: is it cheaper to craft this ingredient than buy it?
        // Track best candidate separately so losing sub-recipes don't leak
        // their sub_crafts into the final breakdown.
        let mut unit_cost = line.unit_price;
        let mut best_sub_crafts: Vec<SubcraftInfo> = Vec::new();
        let mut best_unpriced: u16 = 0;
        if depth < opts.max_subcraft_depth
            && line.used_from_market > 0
            && let Some(sub_recipes) = recipes_by_output.get(&item_id)
        {
            for sub in sub_recipes {
                let sub_breakdown =
                    compute_cost_inner(sub, prices, recipes_by_output, opts, is_shard, depth + 1);
                // sub_breakdown.cost is the total cost of one execution of the
                // sub-recipe, which yields `amount_result` units. Divide by the
                // yield to get a per-unit comparable price.
                let yield_per_craft = sub.amount_result.max(1);
                let sub_unit = sub_breakdown.cost / yield_per_craft;
                // A line no listing priced (unit_cost == 0) is rescued by any
                // priced sub-recipe: an unlisted intermediate is not free when
                // it is craftable. A 0-cost sub-run never wins (it would only
                // relabel "unpriced" as "sub-craft").
                if sub_unit > 0 && (unit_cost == 0 || sub_unit < unit_cost) {
                    unit_cost = sub_unit;
                    let mut winner = sub_breakdown.sub_crafts;
                    winner.push(SubcraftInfo {
                        item_id,
                        amount: line.used_from_market,
                        unit_cost: sub_unit,
                    });
                    best_sub_crafts = winner;
                    best_unpriced = sub_breakdown.unpriced_market_lines;
                }
            }
            if !best_sub_crafts.is_empty() {
                line.unit_price = unit_cost;
                line.source = PriceSource::Subcraft;
                line.world_id = 0;
            }
            sub_crafts.extend(best_sub_crafts.into_iter());
        }
        unpriced = unpriced.saturating_add(best_unpriced);

        let line_market_cost = (line.used_from_market as i64) * (unit_cost as i64);
        // On-hand is valued at the same unit cost as the market portion — i.e.
        // "what would I have paid to acquire this if I didn't already have it",
        // which is the cheapest of market/subcraft after the search above.
        let line_on_hand_value = (line.used_from_on_hand as i64) * (line.unit_price as i64);

        if line.is_shard {
            // Shards always contribute to shard_cost (full replacement value)
            // so the UI can show "shards excluded: Xg". Whether they contribute
            // to the headline cost depends on the mode.
            shard_cost = shard_cost.saturating_add(line_market_cost + line_on_hand_value);
            if matches!(opts.shards, ShardsMode::IncludeMarket) {
                cost = cost.saturating_add(line_market_cost);
                on_hand_savings = on_hand_savings.saturating_add(line_on_hand_value);
            }
            // ExcludeShards: shard on-hand savings intentionally excluded from the
            // aggregate savings — shards are "off the books" entirely.
        } else {
            cost = cost.saturating_add(line_market_cost);
            on_hand_savings = on_hand_savings.saturating_add(line_on_hand_value);
        }

        // Counted after the shard flag and the sub-craft pass: a rescued line
        // is `Subcraft` by now, an excluded shard is off the books, and an
        // item the vendor sells is never "unpriced" even when require_hq
        // skipped its vendor floor.
        let vendor_sold = opts
            .vendor_prices
            .and_then(|m| m.get(&item_id.0))
            .is_some_and(|p| *p > 0);
        let off_the_books = line.is_shard && matches!(opts.shards, ShardsMode::ExcludeShards);
        if line.source == PriceSource::Market
            && line.used_from_market > 0
            && line.unit_price == 0
            && !off_the_books
            && !vendor_sold
        {
            unpriced = unpriced.saturating_add(1);
        }

        ingredient_lines.push(line);
    }

    CostBreakdown {
        cost: clamp_i64_to_i32(cost),
        shard_cost: clamp_i64_to_i32(shard_cost),
        on_hand_savings: clamp_i64_to_i32(on_hand_savings),
        ingredient_lines,
        sub_crafts,
        unpriced_market_lines: unpriced,
    }
}

/// Saturating cast from i64 accumulator to i32 field. Promoted from a closure
/// so Task 4's `compute_cost_inner` can reuse it without duplicating logic.
fn clamp_i64_to_i32(v: i64) -> i32 {
    if v < 0 {
        0
    } else if v > i32::MAX as i64 {
        i32::MAX
    } else {
        v as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_on_hand_returns_zero() {
        let oh = EmptyOnHand;
        assert_eq!(oh.available(ItemId(1)), 0);
    }

    #[test]
    fn shards_mode_default_is_exclude() {
        assert_eq!(ShardsMode::default(), ShardsMode::ExcludeShards);
    }

    #[test]
    fn item_page_default_options_match_existing_behavior() {
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions::item_page_default(&oh);
        assert!(!opts.require_hq);
        assert_eq!(opts.max_subcraft_depth, 0);
        assert_eq!(opts.shards, ShardsMode::ExcludeShards);
    }

    use std::cell::Cell;
    use ultros_api_types::cheapest_listings::{
        CheapestListingItem, CheapestListings, CheapestListingsMap,
    };

    /// Build a CheapestListingsMap with one (item_id, hq) -> price entry.
    fn one_listing(item_id: i32, hq: bool, price: i32, world_id: i32) -> CheapestListingsMap {
        let listings = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id,
                hq,
                world_id,
                cheapest_price: price,
            }],
        };
        CheapestListingsMap::from(listings)
    }

    // --- `compute_ingredient_cost` unit tests ---

    struct MockOnHand {
        available: Cell<i32>,
        consumed: Cell<i32>,
    }
    impl MockOnHand {
        fn new(qty: i32) -> Self {
            Self {
                available: Cell::new(qty),
                consumed: Cell::new(0),
            }
        }
    }
    impl OnHand for MockOnHand {
        fn available(&self, _item: ItemId) -> i32 {
            self.available.get()
        }
        fn consume(&self, _item: ItemId, qty: i32) {
            self.consumed.set(self.consumed.get() + qty);
            self.available.set(self.available.get() - qty);
        }
    }

    #[test]
    fn compute_ingredient_cost_no_on_hand() {
        let prices = one_listing(100, false, 50, 1);
        let on_hand = MockOnHand::new(0);
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &on_hand,
            vendor_prices: None,
        };

        let line = compute_ingredient_cost(ItemId(100), 10, &prices, &opts);

        assert_eq!(line.used_from_on_hand, 0);
        assert_eq!(line.used_from_market, 10);
        assert_eq!(line.unit_price, 50);
        assert_eq!(on_hand.consumed.get(), 0);
    }

    #[test]
    fn compute_ingredient_cost_partial_on_hand() {
        let prices = one_listing(100, false, 50, 1);
        let on_hand = MockOnHand::new(4);
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &on_hand,
            vendor_prices: None,
        };

        let line = compute_ingredient_cost(ItemId(100), 10, &prices, &opts);

        assert_eq!(line.used_from_on_hand, 4);
        assert_eq!(line.used_from_market, 6);
        assert_eq!(line.unit_price, 50);
        assert_eq!(on_hand.consumed.get(), 4);
    }

    #[test]
    fn compute_ingredient_cost_full_on_hand() {
        let prices = one_listing(100, false, 50, 1);
        let on_hand = MockOnHand::new(20);
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &on_hand,
            vendor_prices: None,
        };

        let line = compute_ingredient_cost(ItemId(100), 10, &prices, &opts);

        assert_eq!(line.used_from_on_hand, 10);
        assert_eq!(line.used_from_market, 0);
        assert_eq!(line.unit_price, 50);
        assert_eq!(on_hand.consumed.get(), 10);
        assert_eq!(on_hand.available.get(), 10); // Check that remaining amount is correct
    }

    #[test]
    fn compute_ingredient_cost_hq_fallback() {
        // Require HQ, but only LQ is available
        let prices = one_listing(100, false, 50, 1);
        let on_hand = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: true,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &on_hand,
            vendor_prices: None,
        };

        let line = compute_ingredient_cost(ItemId(100), 10, &prices, &opts);

        assert_eq!(line.unit_price, 50); // Falls back to LQ
    }

    #[test]
    fn compute_ingredient_cost_hq_preference() {
        // Both HQ and LQ available, require_hq = true should pick HQ price
        let prices = two_listings((100, false, 30), (100, true, 80), 1);
        let on_hand = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: true,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &on_hand,
            vendor_prices: None,
        };

        let line = compute_ingredient_cost(ItemId(100), 10, &prices, &opts);

        assert_eq!(line.unit_price, 80); // Picks HQ
    }

    fn two_listings(
        a: (i32, bool, i32),
        b: (i32, bool, i32),
        world_id: i32,
    ) -> CheapestListingsMap {
        let listings = CheapestListings {
            cheapest_listings: vec![
                CheapestListingItem {
                    item_id: a.0,
                    hq: a.1,
                    world_id,
                    cheapest_price: a.2,
                },
                CheapestListingItem {
                    item_id: b.0,
                    hq: b.1,
                    world_id,
                    cheapest_price: b.2,
                },
            ],
        };
        CheapestListingsMap::from(listings)
    }

    /// Mutable on-hand wrapper for tests.
    struct MapOnHand {
        inner: std::collections::HashMap<i32, Cell<i32>>,
    }
    impl MapOnHand {
        fn new(pairs: &[(i32, i32)]) -> Self {
            Self {
                inner: pairs.iter().map(|(id, q)| (*id, Cell::new(*q))).collect(),
            }
        }
    }
    impl OnHand for MapOnHand {
        fn available(&self, item: ItemId) -> i32 {
            self.inner.get(&item.0).map(|c| c.get()).unwrap_or(0)
        }
        fn consume(&self, item: ItemId, qty: i32) {
            if let Some(c) = self.inner.get(&item.0) {
                c.set((c.get() - qty).max(0));
            }
        }
    }

    #[test]
    fn ingredient_cost_basic_lq() {
        let prices = one_listing(100, false, 50, 1);
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
            vendor_prices: None,
        };
        let line = compute_ingredient_cost(ItemId(100), 10, &prices, &opts);
        assert_eq!(line.needed_total, 10);
        assert_eq!(line.used_from_on_hand, 0);
        assert_eq!(line.used_from_market, 10);
        assert_eq!(line.unit_price, 50);
        assert!(!line.is_shard);
    }

    #[test]
    fn ingredient_cost_on_hand_clamps_to_need() {
        let prices = one_listing(100, false, 50, 1);
        let oh = MapOnHand::new(&[(100, 999)]);
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
            vendor_prices: None,
        };
        let line = compute_ingredient_cost(ItemId(100), 10, &prices, &opts);
        assert_eq!(line.used_from_on_hand, 10);
        assert_eq!(line.used_from_market, 0);
        assert_eq!(oh.available(ItemId(100)), 989);
    }

    #[test]
    fn ingredient_cost_on_hand_partial() {
        let prices = one_listing(100, false, 50, 1);
        let oh = MapOnHand::new(&[(100, 3)]);
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
            vendor_prices: None,
        };
        let line = compute_ingredient_cost(ItemId(100), 10, &prices, &opts);
        assert_eq!(line.used_from_on_hand, 3);
        assert_eq!(line.used_from_market, 7);
        assert_eq!(oh.available(ItemId(100)), 0);
    }

    #[test]
    fn ingredient_cost_hq_preferred_with_fallback() {
        let prices = two_listings((100, true, 100), (100, false, 50), 1);
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: true,
            max_subcraft_depth: 0,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
            vendor_prices: None,
        };
        let line = compute_ingredient_cost(ItemId(100), 1, &prices, &opts);
        assert_eq!(line.unit_price, 100);
    }

    #[test]
    fn ingredient_cost_hq_falls_back_to_lq_when_no_hq_listing() {
        let prices = one_listing(100, false, 50, 1);
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: true,
            max_subcraft_depth: 0,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
            vendor_prices: None,
        };
        let line = compute_ingredient_cost(ItemId(100), 1, &prices, &opts);
        assert_eq!(line.unit_price, 50);
    }

    fn vendor_map(entries: &[(i32, i32)]) -> HashMap<i32, i32> {
        entries.iter().copied().collect()
    }

    #[test]
    fn vendor_price_undercuts_market() {
        let prices = one_listing(100, false, 50, 1);
        let vendors = vendor_map(&[(100, 20)]);
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: Some(&vendors),
        };
        let line = compute_ingredient_cost(ItemId(100), 3, &prices, &opts);
        assert_eq!(line.unit_price, 20);
        assert_eq!(line.source, PriceSource::Vendor);
    }

    #[test]
    fn market_price_undercuts_vendor() {
        let prices = one_listing(100, false, 50, 1);
        let vendors = vendor_map(&[(100, 80)]);
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: Some(&vendors),
        };
        let line = compute_ingredient_cost(ItemId(100), 3, &prices, &opts);
        assert_eq!(line.unit_price, 50);
        assert_eq!(line.source, PriceSource::Market);
    }

    #[test]
    fn vendor_price_used_when_no_listing() {
        let prices = one_listing(999, false, 50, 1); // listing for a different item
        let vendors = vendor_map(&[(100, 20)]);
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: Some(&vendors),
        };
        let line = compute_ingredient_cost(ItemId(100), 1, &prices, &opts);
        assert_eq!(line.unit_price, 20);
        assert_eq!(line.source, PriceSource::Vendor);
    }

    #[test]
    fn zero_vendor_price_is_ignored() {
        let prices = one_listing(100, false, 50, 1);
        let vendors = vendor_map(&[(100, 0)]);
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: Some(&vendors),
        };
        let line = compute_ingredient_cost(ItemId(100), 1, &prices, &opts);
        assert_eq!(line.unit_price, 50);
        assert_eq!(line.source, PriceSource::Market);
    }

    #[test]
    fn require_hq_ignores_vendor_floor() {
        // HQ listing at 100, vendor at 20: an HQ-required ingredient must not be
        // silently downgraded to the NQ vendor copy.
        let prices = one_listing(100, true, 100, 1);
        let vendors = vendor_map(&[(100, 20)]);
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: true,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: Some(&vendors),
        };
        let line = compute_ingredient_cost(ItemId(100), 1, &prices, &opts);
        assert_eq!(line.unit_price, 100);
        assert_eq!(line.source, PriceSource::Market);
    }

    use crate::components::crafting_cost::fixtures::*;
    use xiv_gen::Recipe;

    fn make_recipe(ingredients: &[(i32, i32)]) -> Recipe {
        make_recipe_yielding(ingredients, 0, 1)
    }

    fn make_recipe_yielding(
        ingredients: &[(i32, i32)],
        item_result: i32,
        yield_qty: i32,
    ) -> Recipe {
        // Recipe in xiv_gen has fixed-size arrays for ingredient[8] and amount_ingredient[8].
        let mut ing = [0i32; 8];
        let mut amt = [0i32; 8];
        for (i, (id, q)) in ingredients.iter().enumerate() {
            ing[i] = *id;
            amt[i] = *q;
        }
        Recipe {
            key_id: xiv_gen::RecipeId::default(),
            item_result,
            amount_result: yield_qty,
            ingredient: ing,
            amount_ingredient: amt,
            craft_type: 0,
            recipe_level_table: 0,
        }
    }

    #[test]
    fn compute_cost_simple_recipe_lq() {
        let prices = fixture_simple_recipe_prices();
        let cats = fixture_categories();
        let recipe = make_recipe(&[(1000, 2)]);
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: None,
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let cb = compute_cost(&recipe, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.cost, 200);
        assert_eq!(cb.shard_cost, 0);
    }

    #[test]
    fn compute_cost_excludes_shards_by_default() {
        let prices = fixture_shard_recipe_prices();
        let cats = fixture_categories();
        let recipe = make_recipe(&[(1000, 2), (1001, 5)]);
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: None,
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let cb = compute_cost(&recipe, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.cost, 200);
        assert_eq!(cb.shard_cost, 25);
        assert_eq!(cb.ingredient_lines.len(), 2);
        assert!(cb.ingredient_lines.iter().any(|l| l.is_shard));
    }

    #[test]
    fn compute_cost_includes_shards_when_requested() {
        let prices = fixture_shard_recipe_prices();
        let cats = fixture_categories();
        let recipe = make_recipe(&[(1000, 2), (1001, 5)]);
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
            vendor_prices: None,
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let cb = compute_cost(&recipe, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.cost, 225); // 200 + 25
        assert_eq!(cb.shard_cost, 25);
    }

    #[test]
    fn compute_cost_on_hand_savings() {
        let prices = fixture_simple_recipe_prices();
        let cats = fixture_categories();
        let recipe = make_recipe(&[(1000, 2)]);
        let oh = MapOnHand::new(&[(1000, 1)]);
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: None,
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let cb = compute_cost(&recipe, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.cost, 100);
        assert_eq!(cb.on_hand_savings, 100);
    }

    #[test]
    fn compute_cost_subcraft_termination() {
        // Pathological: ingredient 1000 has a recipe that needs ingredient 2000,
        // and ingredient 2000 has a recipe that needs ingredient 1000.
        // max_subcraft_depth=2 must terminate.
        let prices = fixture_simple_recipe_prices();
        let cats = fixture_categories();

        let outer = make_recipe(&[(1000, 1)]);
        let inner_a = make_recipe(&[(2000, 1)]);
        let inner_b = make_recipe(&[(1000, 1)]);

        // The `&'static Recipe` requirement on recipes_by_output makes
        // this awkward in tests. Use Box::leak to fake-static the test data.
        let leaked_inner_a: &'static Recipe = Box::leak(Box::new(inner_a));
        let leaked_inner_b: &'static Recipe = Box::leak(Box::new(inner_b));
        let mut recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        recipes_by_output.insert(ItemId(1000), vec![leaked_inner_a]);
        recipes_by_output.insert(ItemId(2000), vec![leaked_inner_b]);

        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 2,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
            vendor_prices: None,
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);

        let cb = compute_cost(&outer, &prices, &recipes_by_output, &opts, &is_shard);
        // Just verify termination: cost is finite and non-panic.
        assert!(cb.cost >= 0 && cb.cost < i32::MAX);
    }

    #[test]
    fn compute_cost_prefers_subcraft_when_cheaper() {
        // Outer recipe needs 1x item 2000 (priced @ 50g).
        // Sub-recipe: item 2000 from 1x item 1000 (priced @ 30g).
        // With subcrafts enabled, cost should be 30 not 50.
        let prices = CheapestListingsMap::from(CheapestListings {
            cheapest_listings: vec![
                CheapestListingItem {
                    item_id: 1000,
                    hq: false,
                    world_id: 1,
                    cheapest_price: 30,
                },
                CheapestListingItem {
                    item_id: 2000,
                    hq: false,
                    world_id: 1,
                    cheapest_price: 50,
                },
            ],
        });
        let cats = fixture_categories();

        let outer = make_recipe(&[(2000, 1)]);
        let inner = make_recipe(&[(1000, 1)]);
        let leaked: &'static Recipe = Box::leak(Box::new(inner));
        let mut recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        recipes_by_output.insert(ItemId(2000), vec![leaked]);

        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 2,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
            vendor_prices: None,
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);

        let cb = compute_cost(&outer, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.cost, 30);
        assert_eq!(cb.sub_crafts.len(), 1);
        assert_eq!(cb.sub_crafts[0].item_id, ItemId(2000));
    }

    #[test]
    fn ingredient_line_records_the_chosen_listing_world() {
        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: None,
        };
        // NQ on world 7, HQ (dearer) on world 9: lowest picks NQ's world.
        let both = CheapestListingsMap::from(CheapestListings {
            cheapest_listings: vec![
                CheapestListingItem {
                    item_id: 1000,
                    hq: false,
                    world_id: 7,
                    cheapest_price: 100,
                },
                CheapestListingItem {
                    item_id: 1000,
                    hq: true,
                    world_id: 9,
                    cheapest_price: 150,
                },
            ],
        });
        assert_eq!(
            compute_ingredient_cost(ItemId(1000), 1, &both, &opts).world_id,
            7
        );
        let hq_opts = CraftingCostOptions {
            require_hq: true,
            ..opts_copy(&opts, &oh)
        };
        assert_eq!(
            compute_ingredient_cost(ItemId(1000), 1, &both, &hq_opts).world_id,
            9
        );
        // No listing at all: world 0 and price 0.
        let none = CheapestListingsMap::from(CheapestListings {
            cheapest_listings: vec![],
        });
        let line = compute_ingredient_cost(ItemId(1000), 1, &none, &opts);
        assert_eq!((line.unit_price, line.world_id), (0, 0));
        // A cheaper vendor wins: the world is 0 because nothing is bought on a market.
        let mut vendors = HashMap::new();
        vendors.insert(1000, 40);
        let vendor_opts = CraftingCostOptions {
            vendor_prices: Some(&vendors),
            ..opts_copy(&opts, &oh)
        };
        let line = compute_ingredient_cost(ItemId(1000), 1, &both, &vendor_opts);
        assert_eq!((line.source, line.world_id), (PriceSource::Vendor, 0));
    }

    /// `CraftingCostOptions` holds a `&dyn OnHand`, so it is neither `Copy`
    /// nor `Clone`; rebuild it field by field.
    fn opts_copy<'a>(o: &CraftingCostOptions<'a>, oh: &'a dyn OnHand) -> CraftingCostOptions<'a> {
        CraftingCostOptions {
            require_hq: o.require_hq,
            max_subcraft_depth: o.max_subcraft_depth,
            shards: o.shards,
            on_hand: oh,
            vendor_prices: o.vendor_prices,
        }
    }

    #[test]
    fn zero_priced_line_can_be_rescued_by_subcraft() {
        // Outer needs 1x 2000, which has NO listing; 2000 is craftable from 1x 1000 @ 100.
        let prices = one_listing(1000, false, 100, 1);
        let cats = fixture_categories();
        let outer = make_recipe(&[(2000, 1)]);
        let inner: &'static Recipe =
            Box::leak(Box::new(make_recipe_yielding(&[(1000, 1)], 2000, 1)));
        let mut recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        recipes_by_output.insert(ItemId(2000), vec![inner]);
        let oh = EmptyOnHand;
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let with_subs = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 2,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: None,
        };
        let cb = compute_cost(&outer, &prices, &recipes_by_output, &with_subs, &is_shard);
        assert_eq!(
            cb.cost, 100,
            "the unlisted intermediate is costed as a craft, not as free"
        );
        assert_eq!(cb.ingredient_lines[0].source, PriceSource::Subcraft);
        assert_eq!(cb.ingredient_lines[0].world_id, 0);
        assert_eq!(cb.unpriced_market_lines, 0);
        // Sub-crafts off: still free, still counted as unpriced.
        let no_subs = CraftingCostOptions {
            max_subcraft_depth: 0,
            ..opts_copy(&with_subs, &oh)
        };
        let cb = compute_cost(&outer, &prices, &recipes_by_output, &no_subs, &is_shard);
        assert_eq!((cb.cost, cb.unpriced_market_lines), (0, 1));
        // An all-unpriced sub-recipe cannot rescue anything (the `sub_unit > 0` guard).
        let inner_unpriced: &'static Recipe =
            Box::leak(Box::new(make_recipe_yielding(&[(3000, 1)], 2000, 1)));
        let mut by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        by_output.insert(ItemId(2000), vec![inner_unpriced]);
        let cb = compute_cost(&outer, &prices, &by_output, &with_subs, &is_shard);
        assert_eq!(cb.ingredient_lines[0].source, PriceSource::Market);
        assert_eq!((cb.cost, cb.unpriced_market_lines), (0, 1));
    }

    #[test]
    fn unpriced_lines_counted_after_shard_flag_and_subcraft_pass() {
        // Outer: 1x 1000 (@100), 1x 1001 (shard, unlisted), 1x 2000 (unlisted, craftable
        // from 1x 1000 @100 + 1x 3000 unlisted).
        let prices = one_listing(1000, false, 100, 1);
        let cats = fixture_categories();
        let outer = make_recipe(&[(1000, 1), (1001, 1), (2000, 1)]);
        let inner: &'static Recipe = Box::leak(Box::new(make_recipe_yielding(
            &[(1000, 1), (3000, 1)],
            2000,
            1,
        )));
        let mut by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        by_output.insert(ItemId(2000), vec![inner]);
        let oh = EmptyOnHand;
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 2,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: None,
        };
        let cb = compute_cost(&outer, &prices, &by_output, &opts, &is_shard);
        // The shard is excluded; 2000 was rescued (sub cost 100 > 0); the sub-run's
        // own unpriced 3000 propagates from the winning sub-run.
        assert_eq!(cb.cost, 200);
        assert_eq!(cb.unpriced_market_lines, 1);
    }

    #[test]
    fn unpriced_ignores_excluded_shards_and_vendor_sold() {
        let prices = one_listing(1000, false, 100, 1);
        let cats = fixture_categories();
        let outer = make_recipe(&[(1000, 1), (1001, 1), (2000, 1)]);
        let by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let oh = EmptyOnHand;
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let mut vendors = HashMap::new();
        vendors.insert(2000, 25);
        let excl = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
            vendor_prices: Some(&vendors),
        };
        // Shard excluded, 2000 vendor-sold: nothing is unpriced.
        let cb = compute_cost(&outer, &prices, &by_output, &excl, &is_shard);
        assert_eq!(cb.unpriced_market_lines, 0);
        // Shards on the books: the unlisted shard counts.
        let incl = CraftingCostOptions {
            shards: ShardsMode::IncludeMarket,
            ..opts_copy(&excl, &oh)
        };
        let cb = compute_cost(&outer, &prices, &by_output, &incl, &is_shard);
        assert_eq!(cb.unpriced_market_lines, 1);
        // require_hq skips the vendor floor, but a vendor-sold item is still not "unpriced".
        let hq = CraftingCostOptions {
            require_hq: true,
            ..opts_copy(&excl, &oh)
        };
        let cb = compute_cost(&outer, &prices, &by_output, &hq, &is_shard);
        assert_eq!(cb.ingredient_lines[2].source, PriceSource::Market);
        assert_eq!(cb.ingredient_lines[2].unit_price, 0);
        assert_eq!(cb.unpriced_market_lines, 0);
    }

    #[test]
    fn compute_cost_subcraft_disabled_when_depth_zero() {
        let prices = CheapestListingsMap::from(CheapestListings {
            cheapest_listings: vec![
                CheapestListingItem {
                    item_id: 1000,
                    hq: false,
                    world_id: 1,
                    cheapest_price: 30,
                },
                CheapestListingItem {
                    item_id: 2000,
                    hq: false,
                    world_id: 1,
                    cheapest_price: 50,
                },
            ],
        });
        let cats = fixture_categories();

        let outer = make_recipe(&[(2000, 1)]);
        let inner = make_recipe(&[(1000, 1)]);
        let leaked: &'static Recipe = Box::leak(Box::new(inner));
        let mut recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        recipes_by_output.insert(ItemId(2000), vec![leaked]);

        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
            vendor_prices: None,
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);

        let cb = compute_cost(&outer, &prices, &recipes_by_output, &opts, &is_shard);
        // Depth=0 means no recursion — pay market price of 50.
        assert_eq!(cb.cost, 50);
        assert_eq!(cb.sub_crafts.len(), 0);
    }

    #[test]
    fn compute_cost_subcraft_divides_by_recipe_yield() {
        // Outer needs 1x item 2000 (market @ 50g).
        // Sub-recipe makes 3 of item 2000 from 1x item 1000 (market @ 30g).
        // Per-craft sub cost = 30g; per-unit sub cost = 30/3 = 10g.
        // Cheapest path: subcraft at 10g/unit, total cost for 1 unit = 10g.
        let prices = CheapestListingsMap::from(CheapestListings {
            cheapest_listings: vec![
                CheapestListingItem {
                    item_id: 1000,
                    hq: false,
                    world_id: 1,
                    cheapest_price: 30,
                },
                CheapestListingItem {
                    item_id: 2000,
                    hq: false,
                    world_id: 1,
                    cheapest_price: 50,
                },
            ],
        });
        let cats = fixture_categories();

        let outer = make_recipe(&[(2000, 1)]);
        let inner = make_recipe_yielding(&[(1000, 1)], 2000, 3);
        let leaked: &'static Recipe = Box::leak(Box::new(inner));
        let mut recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        recipes_by_output.insert(ItemId(2000), vec![leaked]);

        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 2,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
            vendor_prices: None,
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);

        let cb = compute_cost(&outer, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.cost, 10);
        assert_eq!(cb.sub_crafts.len(), 1);
        assert_eq!(cb.sub_crafts[0].unit_cost, 10);
    }

    #[test]
    fn compute_cost_on_hand_savings_use_subcraft_cost_when_cheaper() {
        // Outer needs 2x item 2000 (market @ 50g, subcraft makes 1 from 1000@30g).
        // 1 unit on-hand. The 1 market unit costs 30g (subcraft).
        // On-hand savings should also reflect the subcraft cost (30g, not 50g).
        let prices = CheapestListingsMap::from(CheapestListings {
            cheapest_listings: vec![
                CheapestListingItem {
                    item_id: 1000,
                    hq: false,
                    world_id: 1,
                    cheapest_price: 30,
                },
                CheapestListingItem {
                    item_id: 2000,
                    hq: false,
                    world_id: 1,
                    cheapest_price: 50,
                },
            ],
        });
        let cats = fixture_categories();

        let outer = make_recipe(&[(2000, 2)]);
        let inner = make_recipe_yielding(&[(1000, 1)], 2000, 1);
        let leaked: &'static Recipe = Box::leak(Box::new(inner));
        let mut recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        recipes_by_output.insert(ItemId(2000), vec![leaked]);

        let oh = MapOnHand::new(&[(2000, 1)]);
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 2,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
            vendor_prices: None,
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);

        let cb = compute_cost(&outer, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.cost, 30); // 1 market unit at subcraft cost
        assert_eq!(cb.on_hand_savings, 30); // on-hand valued at the same subcraft cost
    }

    #[test]
    fn compute_cost_subcraft_keeps_only_winning_sub_crafts() {
        // Two sub-recipes for item 2000: one expensive (40g), one cheap (20g).
        // sub_crafts should contain only the winner's entry, not both candidates.
        let prices = CheapestListingsMap::from(CheapestListings {
            cheapest_listings: vec![
                CheapestListingItem {
                    item_id: 1000,
                    hq: false,
                    world_id: 1,
                    cheapest_price: 20,
                },
                CheapestListingItem {
                    item_id: 1100,
                    hq: false,
                    world_id: 1,
                    cheapest_price: 40,
                },
                CheapestListingItem {
                    item_id: 2000,
                    hq: false,
                    world_id: 1,
                    cheapest_price: 50,
                },
            ],
        });
        let cats = fixture_categories();

        let outer = make_recipe(&[(2000, 1)]);
        let expensive = make_recipe_yielding(&[(1100, 1)], 2000, 1);
        let cheap = make_recipe_yielding(&[(1000, 1)], 2000, 1);
        let leaked_expensive: &'static Recipe = Box::leak(Box::new(expensive));
        let leaked_cheap: &'static Recipe = Box::leak(Box::new(cheap));
        let mut recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        recipes_by_output.insert(ItemId(2000), vec![leaked_expensive, leaked_cheap]);

        let oh = EmptyOnHand;
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 2,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
            vendor_prices: None,
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);

        let cb = compute_cost(&outer, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.cost, 20);
        // Only the winning sub-recipe contributes a SubcraftInfo.
        assert_eq!(cb.sub_crafts.len(), 1);
        assert_eq!(cb.sub_crafts[0].unit_cost, 20);
    }

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

        let view = SignalView {
            over: None,
            base: &listings,
            stats: Some((&index, SaleStat::Median)),
        };
        let priced = compute_cost(&recipe, &view, &by_output, &opts, &not_shard);
        assert_eq!(priced.cost, 120);
    }
}
