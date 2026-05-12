//! Crafting cost types and computation — scaffolded in Task 1, implemented in Tasks 2-4.
//! The entire module is scaffolding; suppress dead_code until consumers exist.
// TODO(Task 2): remove this allow once compute_ingredient_cost gains a real caller.
#![allow(dead_code)]

use std::collections::HashMap;
use ultros_api_types::cheapest_listings::CheapestListingsMap;
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

pub struct CraftingCostOptions<'a> {
    pub require_hq: bool,
    pub max_subcraft_depth: u8,
    pub shards: ShardsMode,
    pub on_hand: &'a dyn OnHand,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubcraftInfo {
    pub item_id: ItemId,
    pub amount: i32,
    pub unit_cost: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CostBreakdown {
    pub hq_cost: i32,
    pub lq_cost: i32,
    pub shard_cost: i32,
    pub on_hand_savings: i32,
    pub ingredient_lines: Vec<IngredientLine>,
    pub sub_crafts: Vec<SubcraftInfo>,
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

pub fn compute_ingredient_cost(
    item_id: ItemId,
    amount_needed: i32,
    prices: &CheapestListingsMap,
    opts: &CraftingCostOptions<'_>,
) -> IngredientLine {
    // Look up price. HQ-preferred when require_hq, with LQ fallback.
    let summary = prices.find_matching_listings(item_id.0);
    let unit_price = if opts.require_hq {
        summary
            .price_preferring_hq()
            .or_else(|| summary.lowest_gil())
            .unwrap_or(0)
    } else {
        summary.lowest_gil().unwrap_or(0)
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

    IngredientLine {
        item_id,
        needed_total: amount_needed,
        used_from_on_hand,
        used_from_market,
        unit_price,
        is_shard,
    }
}

#[cfg(test)]
pub mod fixtures;

/// Compute the cost of one execution of `recipe`.
///
/// `is_shard` returns true for ingredient item ids whose `item_search_category == 59`.
/// In production this is `|id| tracked_data().items.get(&id).map(|i| i.item_search_category == 59).unwrap_or(false)`.
/// In tests this is a closure over a fixture HashMap.
pub fn compute_cost(
    recipe: &Recipe,
    prices: &CheapestListingsMap,
    _recipes_by_output: &HashMap<ItemId, Vec<&'static Recipe>>,
    opts: &CraftingCostOptions<'_>,
    is_shard: &dyn Fn(ItemId) -> bool,
) -> CostBreakdown {
    let mut hq_cost: i64 = 0;
    let mut lq_cost: i64 = 0;
    let mut shard_cost: i64 = 0;
    let mut on_hand_savings: i64 = 0;
    let mut ingredient_lines: Vec<IngredientLine> = Vec::new();
    let sub_crafts: Vec<SubcraftInfo> = Vec::new(); // populated in Task 4

    for (item_id, amount) in IngredientsIter::new(recipe) {
        let mut line = compute_ingredient_cost(item_id, amount, prices, opts);
        line.is_shard = is_shard(item_id);

        let line_market_cost = (line.used_from_market as i64) * (line.unit_price as i64);
        let line_on_hand_value = (line.used_from_on_hand as i64) * (line.unit_price as i64);

        if line.is_shard {
            // Shards are accumulated separately so we can show the user
            // what they "saved" by excluding them.
            shard_cost = shard_cost.saturating_add(line_market_cost + line_on_hand_value);
            if matches!(opts.shards, ShardsMode::IncludeMarket) {
                hq_cost = hq_cost.saturating_add(line_market_cost);
                lq_cost = lq_cost.saturating_add(line_market_cost);
                on_hand_savings = on_hand_savings.saturating_add(line_on_hand_value);
            }
            // ExcludeShards: don't add to hq/lq_cost; still record the line for UI.
        } else {
            hq_cost = hq_cost.saturating_add(line_market_cost);
            lq_cost = lq_cost.saturating_add(line_market_cost);
            on_hand_savings = on_hand_savings.saturating_add(line_on_hand_value);
        }

        ingredient_lines.push(line);
    }

    let clamp = |v: i64| -> i32 {
        if v < 0 {
            0
        } else if v > i32::MAX as i64 {
            i32::MAX
        } else {
            v as i32
        }
    };

    CostBreakdown {
        hq_cost: clamp(hq_cost),
        lq_cost: clamp(lq_cost),
        shard_cost: clamp(shard_cost),
        on_hand_savings: clamp(on_hand_savings),
        ingredient_lines,
        sub_crafts,
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
        };
        let line = compute_ingredient_cost(ItemId(100), 1, &prices, &opts);
        assert_eq!(line.unit_price, 50);
    }

    use crate::components::crafting_cost::fixtures::*;
    use xiv_gen::Recipe;

    fn make_recipe(ingredients: &[(i32, i32)]) -> Recipe {
        // Recipe in xiv_gen has fixed-size arrays for ingredient[8] and amount_ingredient[8].
        let mut ing = [0i32; 8];
        let mut amt = [0i32; 8];
        for (i, (id, q)) in ingredients.iter().enumerate() {
            ing[i] = *id;
            amt[i] = *q;
        }
        Recipe {
            key_id: xiv_gen::RecipeId::default(),
            item_result: 0,
            amount_result: 0,
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
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let cb = compute_cost(&recipe, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.lq_cost, 200);
        assert_eq!(cb.hq_cost, 200);
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
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let cb = compute_cost(&recipe, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.lq_cost, 200);
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
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let cb = compute_cost(&recipe, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.lq_cost, 225); // 200 + 25
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
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let cb = compute_cost(&recipe, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.lq_cost, 100);
        assert_eq!(cb.on_hand_savings, 100);
    }
}
