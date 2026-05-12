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

// Placeholder — implemented in Task 2.
pub fn compute_ingredient_cost(
    _item_id: ItemId,
    _amount_needed: i32,
    _prices: &CheapestListingsMap,
    _opts: &CraftingCostOptions<'_>,
) -> IngredientLine {
    unimplemented!("Task 2")
}

// Placeholder — implemented in Tasks 3-4.
pub fn compute_cost(
    _recipe: &Recipe,
    _prices: &CheapestListingsMap,
    _recipes_by_output: &HashMap<ItemId, Vec<&'static Recipe>>,
    _opts: &CraftingCostOptions<'_>,
) -> CostBreakdown {
    unimplemented!("Tasks 3-4")
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
}
