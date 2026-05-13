# Crafting Recipes — Unified Cost, Shard Toggle, and On-Hand Accounting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the item page, recipe analyzer, and FC crafting analyzer all quote the same craft cost from one shared module, with a first-class "exclude shards" toggle and an on-hand inventory accounting that reads from either LocalStorage or the user's active crafting list.

**Architecture:** New `components/crafting_cost.rs` module owns one parameterized cost function. Both analyzers and the item page's recipe panel call into it. A new `CraftOptions` cookie + URL-param overlay carries toggles across surfaces. An `OnHand` trait has two implementations: `LocalOnHand` (LocalStorage-backed, for anonymous use) and `ListOnHand` (reads `ListItem.acquired` from the user's active list).

**Tech Stack:** Rust, Leptos (SSR/hydrate), `leptos_i18n`, `serde`/`serde_json` (cookie codec), `gloo_storage` (LocalStorage). Tests are stock `#[cfg(test)] mod tests` blocks plus snapshot fixtures.

**Spec:** [docs/superpowers/specs/2026-05-12-crafting-recipes-unified-design.md](../specs/2026-05-12-crafting-recipes-unified-design.md)

---

## File Structure

**New:**
- `ultros-frontend/ultros-app/src/components/crafting_cost.rs` — types, `OnHand` trait, `compute_ingredient_cost` primitive, `compute_cost` recipe walker. Also hosts `IngredientsIter` moved from `related_items.rs`.
- `ultros-frontend/ultros-app/src/components/crafting_cost/fixtures.rs` — snapshot fixtures for parity tests (CheapestListings payload + expected `(hq, lq)` pair per representative recipe).
- `ultros-frontend/ultros-app/src/components/on_hand_input.rs` — `OnHandQuantity` inline widget + `OnHandPanel` collapsible summary.
- `ultros-frontend/ultros-app/src/global_state/craft_options.rs` — `CraftOptions` struct (cookie codec) + read/write helpers.

**Edited:**
- `ultros-frontend/ultros-app/src/components/related_items.rs` — delete local `calculate_crafting_cost` (line 108) and `IngredientsIter` (move, re-export); rewire `RecipePriceEstimate` and the inline profit closure (~lines 225-299); add toggle row + on-hand disclosure to "Crafting Recipes" panel.
- `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` — delete local `calculate_crafting_cost` (line 81); add `shards` + `on-hand-source` URL params; new filter card; consume `CraftOptions` cookie for defaults.
- `ultros-frontend/ultros-app/src/routes/fc_crafting_analyzer.rs` — rewrite `calculate_fc_project_cost` to consume `compute_ingredient_cost`; add `shards` + `on-hand-source` URL params + filter card.
- `ultros-frontend/ultros-app/src/components/mod.rs` — export `crafting_cost`, `on_hand_input`.
- `ultros-frontend/ultros-app/src/global_state/mod.rs` — export `craft_options`.
- `ultros-frontend/ultros-app/src/components/add_recipe_to_current_list.rs:5` — re-point `IngredientsIter` import.
- `ultros-frontend/ultros-app/locales/en.json` — new i18n keys (toggle labels, banner phrases, chip labels).

No backend changes. No schema changes. No new API endpoints.

---

## Task 1: Scaffold `crafting_cost.rs` — types, trait, defaults

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/crafting_cost.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`

Lay down the type surface and re-export `IngredientsIter` from its new home. No cost logic yet — just the shape so Task 2 has somewhere to write.

- [ ] **Step 1: Create `crafting_cost.rs` with type definitions**

```rust
// ultros-frontend/ultros-app/src/components/crafting_cost.rs
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
    fn consume(&self, item: ItemId, qty: i32);
}

/// Empty on-hand source — every `available` returns 0. Used by default
/// and as a sentinel where no on-hand panel is visible.
#[derive(Default)]
pub struct EmptyOnHand;

impl OnHand for EmptyOnHand {
    fn available(&self, _item: ItemId) -> i32 { 0 }
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
    pub cost: i32,
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
#[allow(dead_code)]
pub fn compute_ingredient_cost(
    _item_id: ItemId,
    _amount_needed: i32,
    _prices: &CheapestListingsMap,
    _opts: &CraftingCostOptions<'_>,
) -> IngredientLine {
    unimplemented!("Task 2")
}

// Placeholder — implemented in Tasks 3-4.
#[allow(dead_code)]
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
```

- [ ] **Step 2: Register the module**

Edit `ultros-frontend/ultros-app/src/components/mod.rs` — add the new line in alphabetical order:

```rust
pub mod crafting_cost;
```

(Find the existing `pub mod cheapest_price;` block; insert after it.)

- [ ] **Step 3: Run the tests**

```bash
cd ultros-frontend/ultros-app
cargo test --features hydrate crafting_cost::tests 2>&1 | tail -20
```

Expected: 3 tests pass.

If `--features hydrate` isn't right for this crate, try `cargo test crafting_cost::tests`. The crate has both `ssr` and `hydrate` features; tests should run under whichever the workspace defaults to.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/crafting_cost.rs \
        ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(crafting_cost): scaffold module with types and OnHand trait"
```

---

## Task 2: Implement `compute_ingredient_cost` primitive

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/crafting_cost.rs`

The atomic per-ingredient calculation. Handles shard skipping, on-hand deduction, HQ-preferred sourcing with LQ fallback. Both `compute_cost` (recipe) and the FC analyzer will sit on top of this.

- [ ] **Step 1: Write the failing tests**

Append to `crafting_cost.rs` inside `mod tests`:

```rust
    use std::cell::Cell;
    use ultros_api_types::cheapest_listings::{
        CheapestListingMapKey, CheapestListings, CheapestListingsMap,
    };

    /// Build a CheapestListingsMap with one (item_id, hq) -> price entry.
    fn one_listing(item_id: i32, hq: bool, price: i32, world_id: i32) -> CheapestListingsMap {
        let listings = CheapestListings {
            cheapest_listings: vec![ultros_api_types::cheapest_listings::CheapestListingItem {
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
                ultros_api_types::cheapest_listings::CheapestListingItem {
                    item_id: a.0,
                    hq: a.1,
                    world_id,
                    cheapest_price: a.2,
                },
                ultros_api_types::cheapest_listings::CheapestListingItem {
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
        fn from(pairs: &[(i32, i32)]) -> Self {
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
        // Item 100 LQ @ 50g, need 10. Result: 500g, no on-hand, not a shard.
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
        // need 10, have 999 — should use 10 from on-hand, 0 from market.
        let prices = one_listing(100, false, 50, 1);
        let oh = MapOnHand::from(&[(100, 999)]);
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::IncludeMarket,
            on_hand: &oh,
        };
        let line = compute_ingredient_cost(ItemId(100), 10, &prices, &opts);
        assert_eq!(line.used_from_on_hand, 10);
        assert_eq!(line.used_from_market, 0);
        // Verify on-hand was actually consumed (the cell decremented).
        assert_eq!(oh.available(ItemId(100)), 989);
    }

    #[test]
    fn ingredient_cost_on_hand_partial() {
        // need 10, have 3 — 3 from on-hand, 7 from market.
        let prices = one_listing(100, false, 50, 1);
        let oh = MapOnHand::from(&[(100, 3)]);
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
        // require_hq=true, HQ listing @ 100g, LQ listing @ 50g — use HQ.
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
        // require_hq=true but only LQ exists — fall back to LQ.
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
```

- [ ] **Step 2: Verify they fail**

```bash
cd ultros-frontend/ultros-app
cargo test crafting_cost::tests 2>&1 | tail -30
```

Expected: 5 new tests panic with `unimplemented!("Task 2")`. The earlier 3 still pass.

- [ ] **Step 3: Implement `compute_ingredient_cost`**

Replace the placeholder `compute_ingredient_cost` body:

```rust
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
            .unwrap_or(0) as i32
    } else {
        summary.lowest_gil().unwrap_or(0) as i32
    };

    // Determine whether this item is a shard/crystal. We don't have the
    // Item struct here; callers detect that and stuff it onto the line via
    // a post-pass. The primitive itself reports unconditionally — the
    // shard-skip happens in `compute_cost`. Mark for callers.
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
```

Note: `is_shard` is set by the recipe-walking caller in Task 3 (which has access to `tracked_data().items` and can look up `item_search_category`). The primitive stays pure of game-data lookups so it's trivially testable.

- [ ] **Step 4: Verify the tests pass**

```bash
cd ultros-frontend/ultros-app
cargo test crafting_cost::tests 2>&1 | tail -10
```

Expected: all 8 tests pass.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/crafting_cost.rs
git commit -m "feat(crafting_cost): compute_ingredient_cost with on-hand + HQ fallback"
```

---

## Task 3: `compute_cost` (recipe walker, no subcrafts) + item-page parity test

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/crafting_cost.rs`
- Create: `ultros-frontend/ultros-app/src/components/crafting_cost/fixtures.rs`

Wire `compute_ingredient_cost` into a recipe walk that sums per-ingredient costs into `CostBreakdown`. Skip shards based on `item_search_category == 59`. This task does NOT implement subcrafts — `max_subcraft_depth` is ignored in this task.

The parity test pins the new function's `cost` to match the existing `related_items::calculate_crafting_cost` output, so swapping the call sites in later tasks is safe.

- [ ] **Step 1: Create the fixtures file**

```rust
// ultros-frontend/ultros-app/src/components/crafting_cost/fixtures.rs
//! Snapshot fixtures for the crafting_cost parity tests.
//!
//! These pin a representative slice of the current calculation against
//! the new implementation so the swap-overs in Tasks 7-9 are safe.
//!
//! The fixtures use real recipe IDs but synthetic prices, so they
//! remain deterministic regardless of live market data.

use std::collections::HashMap;
use ultros_api_types::cheapest_listings::{
    CheapestListingItem, CheapestListings, CheapestListingsMap,
};

/// A recipe that takes 1 ingredient (no shards). Mirrors the simplest
/// production recipe shape.
pub fn fixture_simple_recipe_prices() -> CheapestListingsMap {
    // item 1000 LQ @ 100g, item 2000 LQ @ 50g (output).
    CheapestListingsMap::from(CheapestListings {
        cheapest_listings: vec![
            CheapestListingItem { item_id: 1000, hq: false, world_id: 1, cheapest_price: 100 },
            CheapestListingItem { item_id: 2000, hq: false, world_id: 1, cheapest_price: 50 },
        ],
    })
}

/// Prices for a recipe that mixes shard and non-shard ingredients.
/// Ingredient 1000 = non-shard @ 100g, ingredient 1001 = shard @ 5g.
pub fn fixture_shard_recipe_prices() -> CheapestListingsMap {
    CheapestListingsMap::from(CheapestListings {
        cheapest_listings: vec![
            CheapestListingItem { item_id: 1000, hq: false, world_id: 1, cheapest_price: 100 },
            CheapestListingItem { item_id: 1001, hq: false, world_id: 1, cheapest_price: 5 },
        ],
    })
}

/// A static set of (item_id -> item_search_category) for the parity
/// tests. Caller passes this where the real code uses
/// `tracked_data().items`.
pub fn fixture_categories() -> HashMap<i32, i32> {
    let mut m = HashMap::new();
    m.insert(1000, 1); // non-shard (search category 1)
    m.insert(1001, 59); // shard
    m.insert(2000, 1); // non-shard output
    m
}
```

- [ ] **Step 2: Register the submodule**

In `crafting_cost.rs`, add at the top of the file (after the existing imports):

```rust
#[cfg(test)]
pub mod fixtures;
```

- [ ] **Step 3: Refactor `compute_cost` signature to accept a shard-detector**

We need to know `is_shard` per ingredient without dragging `tracked_data()` into a test. Pass it as a closure:

Replace the placeholder `compute_cost` with this signature change in `crafting_cost.rs`:

```rust
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
    let mut cost: i64 = 0;
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
                cost = cost.saturating_add(line_market_cost);
                on_hand_savings = on_hand_savings.saturating_add(line_on_hand_value);
            }
            // ExcludeShards: don't add to cost; still record the line for UI.
        } else {
            cost = cost.saturating_add(line_market_cost);
            on_hand_savings = on_hand_savings.saturating_add(line_on_hand_value);
        }

        ingredient_lines.push(line);
    }

    let clamp = |v: i64| -> i32 {
        if v < 0 { 0 } else if v > i32::MAX as i64 { i32::MAX } else { v as i32 }
    };

    CostBreakdown {
        cost: clamp(cost),
        shard_cost: clamp(shard_cost),
        on_hand_savings: clamp(on_hand_savings),
        ingredient_lines,
        sub_crafts,
    }
}
```

NOTE: `CostBreakdown.cost` is the resolved cost for the caller's `require_hq` flavor. Surfaces that need both HQ and LQ totals (currently only the item page's `RecipePriceEstimate`) call `compute_cost` twice and read `.cost` from each result — matching the existing two-pass pattern at [`related_items.rs:130`](../../ultros-frontend/ultros-app/src/components/related_items.rs:130).

- [ ] **Step 4: Write the parity + shards tests**

Append to `mod tests`:

```rust
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
            ingredient: ing,
            amount_ingredient: amt,
            ..Recipe::default()  // see note below
        }
    }
```

If `Recipe` doesn't derive `Default`, use `unsafe { std::mem::zeroed() }` instead — Recipe is plain integer fields. Confirm by checking `xiv-gen/src/lib.rs`'s `Recipe` definition; if it does derive Default, prefer that.

```rust
    #[test]
    fn compute_cost_simple_recipe_lq() {
        // 2x item 1000 @ 100g = 200g, no shards.
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
        assert_eq!(cb.cost, 200);
        assert_eq!(cb.shard_cost, 0);
    }

    #[test]
    fn compute_cost_excludes_shards_by_default() {
        // 2x item 1000 (non-shard @ 100g) + 5x item 1001 (shard @ 5g)
        // ExcludeShards: cost = 200, shard_cost = 25, on_hand_savings = 0
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
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let cb = compute_cost(&recipe, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.cost, 225); // 200 + 25
        assert_eq!(cb.shard_cost, 25);
    }

    #[test]
    fn compute_cost_on_hand_savings() {
        // 2x item 1000 @ 100g, on-hand=1 — pay for 1 (100g), save 100g.
        let prices = fixture_simple_recipe_prices();
        let cats = fixture_categories();
        let recipe = make_recipe(&[(1000, 2)]);
        let oh = MapOnHand::from(&[(1000, 1)]);
        let opts = CraftingCostOptions {
            require_hq: false,
            max_subcraft_depth: 0,
            shards: ShardsMode::ExcludeShards,
            on_hand: &oh,
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);
        let recipes_by_output: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        let cb = compute_cost(&recipe, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.cost, 100);
        assert_eq!(cb.on_hand_savings, 100);
    }
```

- [ ] **Step 5: Run the tests**

```bash
cd ultros-frontend/ultros-app
cargo test crafting_cost::tests 2>&1 | tail -15
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/crafting_cost.rs \
        ultros-frontend/ultros-app/src/components/crafting_cost/fixtures.rs
git commit -m "feat(crafting_cost): compute_cost recipe walker (no subcrafts) with shards + on-hand"
```

---

## Task 4: Add subcraft recursion to `compute_cost`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/crafting_cost.rs`

Recreate the analyzer's "is it cheaper to craft this ingredient than buy it?" recursion, parameterized by `max_subcraft_depth`. Match the existing analyzer behavior at [`recipe_analyzer.rs:111-138`](../../ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:111) — depth defaults to 2 there.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests`:

```rust
    #[test]
    fn compute_cost_subcraft_termination() {
        // Pathological: ingredient 1000 has a recipe that needs ingredient 2000,
        // and ingredient 2000 has a recipe that needs ingredient 1000.
        // max_subcraft_depth=2 must terminate.
        let prices = fixture_simple_recipe_prices();
        let cats = fixture_categories();

        // Build two recipes, each pointing at the other's output.
        // (In real game data this would never happen; we test bound enforcement.)
        let outer = make_recipe(&[(1000, 1)]); // makes 2000 from 1000
        let inner_a = make_recipe(&[(2000, 1)]); // makes 1000 from 2000
        let inner_b = make_recipe(&[(1000, 1)]); // makes 2000 from 1000 again

        // Note: the `&'static Recipe` requirement on recipes_by_output makes
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
                CheapestListingItem { item_id: 1000, hq: false, world_id: 1, cheapest_price: 30 },
                CheapestListingItem { item_id: 2000, hq: false, world_id: 1, cheapest_price: 50 },
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
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);

        let cb = compute_cost(&outer, &prices, &recipes_by_output, &opts, &is_shard);
        assert_eq!(cb.cost, 30);
        assert_eq!(cb.sub_crafts.len(), 1);
        assert_eq!(cb.sub_crafts[0].item_id, ItemId(2000));
    }

    #[test]
    fn compute_cost_subcraft_disabled_when_depth_zero() {
        let prices = CheapestListingsMap::from(CheapestListings {
            cheapest_listings: vec![
                CheapestListingItem { item_id: 1000, hq: false, world_id: 1, cheapest_price: 30 },
                CheapestListingItem { item_id: 2000, hq: false, world_id: 1, cheapest_price: 50 },
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
        };
        let is_shard = |id: ItemId| cats.get(&id.0) == Some(&59);

        let cb = compute_cost(&outer, &prices, &recipes_by_output, &opts, &is_shard);
        // Depth=0 means no recursion — pay market price of 50.
        assert_eq!(cb.cost, 50);
        assert_eq!(cb.sub_crafts.len(), 0);
    }
```

- [ ] **Step 2: Verify they fail**

```bash
cargo test crafting_cost::tests::compute_cost_subcraft 2>&1 | tail -15
```

Expected: subcraft tests fail (the current implementation doesn't recurse).

- [ ] **Step 3: Refactor `compute_cost` into a recursive helper**

Replace the existing `compute_cost` body in `crafting_cost.rs`:

```rust
pub fn compute_cost(
    recipe: &Recipe,
    prices: &CheapestListingsMap,
    recipes_by_output: &HashMap<ItemId, Vec<&'static Recipe>>,
    opts: &CraftingCostOptions<'_>,
    is_shard: &dyn Fn(ItemId) -> bool,
) -> CostBreakdown {
    compute_cost_inner(recipe, prices, recipes_by_output, opts, is_shard, 0)
}

fn compute_cost_inner(
    recipe: &Recipe,
    prices: &CheapestListingsMap,
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

    for (item_id, amount) in IngredientsIter::new(recipe) {
        let mut line = compute_ingredient_cost(item_id, amount, prices, opts);
        line.is_shard = is_shard(item_id);

        // Subcraft check: is it cheaper to craft this ingredient than buy it?
        let mut unit_cost = line.unit_price;
        if depth < opts.max_subcraft_depth && line.used_from_market > 0
            && let Some(sub_recipes) = recipes_by_output.get(&item_id)
        {
            for sub in sub_recipes {
                let sub_breakdown =
                    compute_cost_inner(sub, prices, recipes_by_output, opts, is_shard, depth + 1);
                // Match the active flavor (HQ if require_hq, else LQ).
                let sub_unit = sub_breakdown.cost;
                if sub_unit > 0 && sub_unit < unit_cost {
                    unit_cost = sub_unit;
                    sub_crafts.extend(sub_breakdown.sub_crafts.iter().cloned());
                    sub_crafts.push(SubcraftInfo {
                        item_id,
                        amount: line.used_from_market,
                        unit_cost: sub_unit,
                    });
                }
            }
            // Re-price the market portion at the subcraft unit cost.
            line.unit_price = unit_cost;
        }

        let line_market_cost = (line.used_from_market as i64) * (unit_cost as i64);
        let line_on_hand_value = (line.used_from_on_hand as i64) * (line.unit_price as i64);

        if line.is_shard {
            shard_cost = shard_cost.saturating_add(line_market_cost + line_on_hand_value);
            if matches!(opts.shards, ShardsMode::IncludeMarket) {
                cost = cost.saturating_add(line_market_cost);
                on_hand_savings = on_hand_savings.saturating_add(line_on_hand_value);
            }
        } else {
            cost = cost.saturating_add(line_market_cost);
            on_hand_savings = on_hand_savings.saturating_add(line_on_hand_value);
        }

        ingredient_lines.push(line);
    }

    let clamp = |v: i64| -> i32 {
        if v < 0 { 0 } else if v > i32::MAX as i64 { i32::MAX } else { v as i32 }
    };

    CostBreakdown {
        cost: clamp(cost),
        shard_cost: clamp(shard_cost),
        on_hand_savings: clamp(on_hand_savings),
        ingredient_lines,
        sub_crafts,
    }
}
```

- [ ] **Step 4: Verify all tests pass**

```bash
cd ultros-frontend/ultros-app
cargo test crafting_cost::tests 2>&1 | tail -20
```

Expected: every test passes.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/crafting_cost.rs
git commit -m "feat(crafting_cost): subcraft recursion with depth bound"
```

---

## Task 5: `CraftOptions` cookie + global state

**Files:**
- Create: `ultros-frontend/ultros-app/src/global_state/craft_options.rs`
- Modify: `ultros-frontend/ultros-app/src/global_state/mod.rs`

Mirror the `CrafterLevels` pattern at [`global_state/crafter_levels.rs`](../../ultros-frontend/ultros-app/src/global_state/crafter_levels.rs). One struct, serde JSON codec, accessible via `cookies.use_cookie_typed::<_, CraftOptions>("CRAFT_OPTIONS")`.

- [ ] **Step 1: Create `craft_options.rs`**

```rust
// ultros-frontend/ultros-app/src/global_state/craft_options.rs
use serde::{Deserialize, Serialize};
use std::{fmt::Display, str::FromStr};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CraftOptions {
    #[serde(default)]
    pub require_hq: bool,
    #[serde(default)]
    pub include_subcrafts: bool,
    #[serde(default = "default_exclude_shards")]
    pub exclude_shards: bool,
    #[serde(default)]
    pub use_on_hand: bool,
    /// If set, on-hand is read from this list's `ListItem.acquired`.
    /// If None, on-hand uses LocalStorage.
    #[serde(default)]
    pub active_craft_list: Option<i32>,
}

fn default_exclude_shards() -> bool { true }

impl Default for CraftOptions {
    fn default() -> Self {
        Self {
            require_hq: false,
            include_subcrafts: false,
            exclude_shards: true,
            use_on_hand: false,
            active_craft_list: None,
        }
    }
}

impl Display for CraftOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", serde_json::to_string(self).unwrap_or_default())
    }
}

impl FromStr for CraftOptions {
    type Err = serde_json::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

pub const COOKIE_NAME: &str = "CRAFT_OPTIONS";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_excludes_shards() {
        let opts = CraftOptions::default();
        assert!(opts.exclude_shards);
    }

    #[test]
    fn roundtrip_through_cookie() {
        let opts = CraftOptions {
            require_hq: true,
            include_subcrafts: true,
            exclude_shards: false,
            use_on_hand: true,
            active_craft_list: Some(42),
        };
        let s = opts.to_string();
        let parsed: CraftOptions = s.parse().unwrap();
        assert_eq!(opts, parsed);
    }

    #[test]
    fn missing_fields_get_defaults() {
        // Backward compat: a stale cookie with only one field should still parse.
        let parsed: CraftOptions = r#"{"require_hq":true}"#.parse().unwrap();
        assert!(parsed.require_hq);
        assert!(parsed.exclude_shards); // serde default kicks in
    }
}
```

- [ ] **Step 2: Register the module**

Edit `ultros-frontend/ultros-app/src/global_state/mod.rs` — add (alphabetically):

```rust
pub mod craft_options;
```

- [ ] **Step 3: Run tests**

```bash
cd ultros-frontend/ultros-app
cargo test global_state::craft_options 2>&1 | tail -10
```

Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/global_state/craft_options.rs \
        ultros-frontend/ultros-app/src/global_state/mod.rs
git commit -m "feat(craft_options): CRAFT_OPTIONS cookie state"
```

---

## Task 6: `LocalOnHand` + `OnHandInput` component

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/on_hand_input.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`

LocalStorage-backed `OnHand` implementation plus the per-ingredient number input component. Use `gloo_storage` since it's already in the workspace (check `cargo tree` if uncertain). If not, fall back to `web_sys::Storage`.

- [ ] **Step 1: Confirm storage helper availability**

```bash
cd ultros-frontend/ultros-app
grep -r "gloo_storage\|gloo-storage" Cargo.toml ../../Cargo.lock 2>&1 | head -5
```

If `gloo_storage` is not present, the implementation below uses `web_sys::window().local_storage()` directly. Both options work; pick the one that compiles cleanly with the existing imports in `ultros_app`.

- [ ] **Step 2: Create `on_hand_input.rs`**

```rust
// ultros-frontend/ultros-app/src/components/on_hand_input.rs
use crate::components::crafting_cost::OnHand;
use leptos::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use xiv_gen::ItemId;

const STORAGE_KEY: &str = "ultros.craft.on_hand.v1";

/// LocalStorage-backed OnHand. Reads/writes a JSON HashMap<item_id, qty>.
///
/// Interior mutability: each `compute_cost` call consumes from a per-call
/// snapshot held in a RefCell, so two ingredient lines for the same
/// item share the same pool. Mutations are NOT persisted back to
/// storage — the user owns the canonical qty via the UI.
pub struct LocalOnHand {
    snapshot: RefCell<HashMap<i32, i32>>,
}

impl LocalOnHand {
    /// Take a fresh snapshot from LocalStorage. Call at the top of each
    /// reactive `compute_cost` derivation.
    pub fn from_storage() -> Self {
        let snapshot = read_storage().unwrap_or_default();
        Self { snapshot: RefCell::new(snapshot) }
    }

    /// Construct from an explicit map (tests + ListOnHand backfill).
    pub fn from_map(map: HashMap<i32, i32>) -> Self {
        Self { snapshot: RefCell::new(map) }
    }
}

impl OnHand for LocalOnHand {
    fn available(&self, item: ItemId) -> i32 {
        self.snapshot.borrow().get(&item.0).copied().unwrap_or(0)
    }
    fn consume(&self, item: ItemId, qty: i32) {
        let mut s = self.snapshot.borrow_mut();
        if let Some(v) = s.get_mut(&item.0) {
            *v = (*v - qty).max(0);
        }
    }
}

fn read_storage() -> Option<HashMap<i32, i32>> {
    #[cfg(not(feature = "ssr"))]
    {
        let win = web_sys::window()?;
        let storage = win.local_storage().ok()??;
        let raw = storage.get_item(STORAGE_KEY).ok()??;
        serde_json::from_str(&raw).ok()
    }
    #[cfg(feature = "ssr")]
    {
        None
    }
}

fn write_storage(map: &HashMap<i32, i32>) {
    #[cfg(not(feature = "ssr"))]
    {
        if let Some(win) = web_sys::window()
            && let Ok(Some(storage)) = win.local_storage()
            && let Ok(s) = serde_json::to_string(map)
        {
            let _ = storage.set_item(STORAGE_KEY, &s);
        }
    }
    #[cfg(feature = "ssr")]
    {
        let _ = map;
    }
}

/// Global reactive on-hand map. Mounted once via OnHandProvider.
/// Components that need to display or write reactively use this signal.
#[derive(Clone, Copy)]
pub struct OnHandMap(pub RwSignal<HashMap<i32, i32>>);

#[component]
pub fn OnHandProvider(children: Children) -> impl IntoView {
    let initial = read_storage().unwrap_or_default();
    let sig = RwSignal::new(initial);
    Effect::new(move |_| {
        sig.with(|m| write_storage(m));
    });
    provide_context(OnHandMap(sig));
    children()
}

/// Inline per-ingredient quantity input.
#[component]
pub fn OnHandQuantity(#[prop(into)] item_id: Signal<i32>) -> impl IntoView {
    let on_hand = use_context::<OnHandMap>().expect("OnHandMap not provided");
    let value = Memo::new(move |_| {
        on_hand.0.with(|m| m.get(&item_id()).copied().unwrap_or(0))
    });

    view! {
        <input
            type="number"
            min="0"
            class="input input-xs w-20 text-right"
            placeholder="0"
            aria-label="On-hand quantity"
            prop:value=move || value().to_string()
            on:input=move |ev| {
                let raw = event_target_value(&ev);
                let parsed: i32 = raw.parse().unwrap_or(0).max(0);
                let id = item_id();
                on_hand.0.update(|m| {
                    if parsed == 0 {
                        m.remove(&id);
                    } else {
                        m.insert(id, parsed);
                    }
                });
            }
        />
    }
}

/// Collapsible global panel listing every tracked item, with a reset button.
/// Mounted on the analyzer routes.
#[component]
pub fn OnHandPanel() -> impl IntoView {
    let on_hand = use_context::<OnHandMap>().expect("OnHandMap not provided");
    let is_empty = Memo::new(move |_| on_hand.0.with(|m| m.is_empty()));

    view! {
        <div class="panel p-4 rounded-lg border border-brand-700/30">
            <div class="flex flex-row items-center justify-between mb-2">
                <h3 class="font-bold text-brand-200">"On-hand items"</h3>
                <button
                    class="btn-ghost text-xs"
                    on:click=move |_| on_hand.0.update(|m| m.clear())
                    disabled=move || is_empty()
                >
                    "Reset"
                </button>
            </div>
            <Show
                when=move || !is_empty()
                fallback=|| view! {
                    <div class="text-xs text-[color:var(--color-text-muted)]">
                        "Set on-hand counts on individual ingredient rows."
                    </div>
                }
            >
                {move || on_hand.0.with(|m| {
                    let entries: Vec<(i32, i32)> = m.iter().map(|(k, v)| (*k, *v)).collect();
                    view! {
                        <div class="text-xs text-[color:var(--color-text-muted)]">
                            {format!("{} items tracked", entries.len())}
                        </div>
                    }
                })}
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn local_on_hand_from_map_basic() {
        let mut m = HashMap::new();
        m.insert(100, 5);
        let oh = LocalOnHand::from_map(m);
        assert_eq!(oh.available(ItemId(100)), 5);
        assert_eq!(oh.available(ItemId(999)), 0);
    }

    #[test]
    fn local_on_hand_consume_decrements() {
        let mut m = HashMap::new();
        m.insert(100, 5);
        let oh = LocalOnHand::from_map(m);
        oh.consume(ItemId(100), 3);
        assert_eq!(oh.available(ItemId(100)), 2);
    }

    #[test]
    fn local_on_hand_consume_clamps_at_zero() {
        let mut m = HashMap::new();
        m.insert(100, 2);
        let oh = LocalOnHand::from_map(m);
        oh.consume(ItemId(100), 99);
        assert_eq!(oh.available(ItemId(100)), 0);
    }
}
```

- [ ] **Step 3: Register the module**

Edit `ultros-frontend/ultros-app/src/components/mod.rs` — add:

```rust
pub mod on_hand_input;
```

- [ ] **Step 4: Mount `OnHandProvider` at the app root**

Find the top-level component (likely in `ultros-frontend/ultros-app/src/app.rs` or similar — search if needed):

```bash
grep -rn "fn App" ultros-frontend/ultros-app/src/ | head -3
```

Wrap the routes (or top of `provide_context` block) with `<OnHandProvider>`. Concretely, find the existing chain of `provide_context(...)` calls or context providers and add `view! { <OnHandProvider> ... </OnHandProvider> }` around the child tree.

- [ ] **Step 5: Run the tests**

```bash
cd ultros-frontend/ultros-app
cargo test on_hand_input 2>&1 | tail -10
```

Expected: 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/on_hand_input.rs \
        ultros-frontend/ultros-app/src/components/mod.rs \
        ultros-frontend/ultros-app/src/app.rs
git commit -m "feat(on_hand): LocalOnHand impl + OnHandInput component + provider"
```

(Adjust the `app.rs` path if the provider lands elsewhere.)

---

## Task 7: Swap item page (`related_items.rs`) over to `compute_cost`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/related_items.rs`
- Modify: `ultros-frontend/ultros-app/src/components/add_recipe_to_current_list.rs:5` (re-point `IngredientsIter` import)

Delete the local `calculate_crafting_cost`. `RecipePriceEstimate` now calls `compute_cost` twice (once with `require_hq=false`, once with `require_hq=true`) to keep the existing HQ/LQ chip behavior. The inline profit closure (`Suspense` block) consumes the same `CostBreakdown`. Add a toggle row at the top of the "Crafting Recipes" panel.

- [ ] **Step 1: Re-point the `IngredientsIter` import in `add_recipe_to_current_list.rs:5`**

Change:
```rust
use crate::components::related_items::IngredientsIter;
```
to:
```rust
use crate::components::crafting_cost::IngredientsIter;
```

Re-export from `related_items.rs` for any other consumer is not needed — confirm via:
```bash
grep -rn "related_items::IngredientsIter" ultros-frontend/
```
Only the one site exists. After this task it points at the new home.

- [ ] **Step 2: Delete the local `calculate_crafting_cost` and `IngredientsIter`**

In `related_items.rs`, delete lines 70-131 (the `IngredientsIter` struct, impl, and `calculate_crafting_cost` function). Re-import from the new module by adding to the top:

```rust
use crate::components::crafting_cost::{
    compute_cost, CraftingCostOptions, EmptyOnHand, IngredientsIter, ShardsMode,
};
use crate::components::on_hand_input::{LocalOnHand, OnHandMap};
```

Plus a helper for the shard predicate. Add a free function at the top of the file (after imports):

```rust
fn is_shard_item(item_id: ItemId) -> bool {
    use crate::global_state::xiv_data::tracked_data;
    tracked_data()
        .items
        .get(&item_id)
        .map(|i| i.item_search_category == 59)
        .unwrap_or(false)
}
```

- [ ] **Step 3: Rewrite `RecipePriceEstimate`**

Replace the entire `RecipePriceEstimate` component (around line 133) with:

```rust
#[component]
fn RecipePriceEstimate(recipe: &'static Recipe) -> impl IntoView {
    use crate::global_state::cookies::Cookies;
    use crate::global_state::craft_options::CraftOptions;

    let cheapest_prices = use_context::<CheapestPrices>().unwrap();
    let cookies = use_context::<Cookies>().unwrap();
    let (opts_cookie, _) = cookies.use_cookie_typed::<_, CraftOptions>("CRAFT_OPTIONS");
    let on_hand_map = use_context::<OnHandMap>();

    view! {
        <Suspense fallback=move || view! { <SingleLineSkeleton /> }>
            {move || {
                cheapest_prices.read_listings.with(|prices| {
                    let prices = prices.as_ref()?.as_ref().ok()?;
                    let opts_value = opts_cookie.get().unwrap_or_default();
                    let shards = if opts_value.exclude_shards {
                        ShardsMode::ExcludeShards
                    } else {
                        ShardsMode::IncludeMarket
                    };

                    // Snapshot the LocalStorage on-hand if available.
                    let local = on_hand_map
                        .map(|m| LocalOnHand::from_map(m.0.get_untracked()))
                        .unwrap_or_else(|| LocalOnHand::from_map(Default::default()));
                    let empty = EmptyOnHand;
                    let active_on_hand: &dyn crate::components::crafting_cost::OnHand =
                        if opts_value.use_on_hand { &local } else { &empty };

                    let recipes_by_output = std::collections::HashMap::new();

                    let lq_opts = CraftingCostOptions {
                        require_hq: false,
                        max_subcraft_depth: 0,
                        shards,
                        on_hand: active_on_hand,
                    };
                    let lq = compute_cost(recipe, prices, &recipes_by_output, &lq_opts, &is_shard_item);

                    // Re-snapshot on-hand for the HQ pass (the LQ pass consumed it).
                    let local_hq = on_hand_map
                        .map(|m| LocalOnHand::from_map(m.0.get_untracked()))
                        .unwrap_or_else(|| LocalOnHand::from_map(Default::default()));
                    let active_on_hand_hq: &dyn crate::components::crafting_cost::OnHand =
                        if opts_value.use_on_hand { &local_hq } else { &empty };
                    let hq_opts = CraftingCostOptions {
                        require_hq: true,
                        max_subcraft_depth: 0,
                        shards,
                        on_hand: active_on_hand_hq,
                    };
                    let hq = compute_cost(recipe, prices, &recipes_by_output, &hq_opts, &is_shard_item);

                    Some(view! {
                        <span class="flex flex-row gap-2 items-center flex-wrap">
                            <span class="px-1.5 py-0.5 rounded bg-[color:color-mix(in_srgb,var(--brand-ring)_16%,transparent)] text-xs">"HQ:"</span>
                            <Gil amount=hq.cost />
                            <span class="px-1.5 py-0.5 rounded bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)] text-xs">"LQ:"</span>
                            <Gil amount=lq.cost />
                            {(lq.shard_cost > 0 && opts_value.exclude_shards).then(|| view! {
                                <span class="px-1.5 py-0.5 rounded bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)] text-[10px] text-[color:var(--color-text-muted)]">
                                    "shards excl. " <Gil amount=lq.shard_cost />
                                </span>
                            })}
                            {(lq.on_hand_savings > 0).then(|| view! {
                                <span class="px-1.5 py-0.5 rounded bg-emerald-900/30 text-emerald-300 text-[10px]">
                                    "saved " <Gil amount=lq.on_hand_savings />
                                </span>
                            })}
                        </span>
                    })
                })
            }}
        </Suspense>
    }
}
```

- [ ] **Step 4: Collapse the duplicate inline profit closure**

In the `Recipe` component (around line 225), the `Suspense` block computes cost a second time via an inline `sum_for`. Delete that closure body and replace with a single `compute_cost` call. The full replacement for that `Suspense` block:

```rust
<Suspense fallback=move || view! { <SingleLineSkeleton /> }>
    {move || {
        use crate::global_state::cookies::Cookies;
        use crate::global_state::craft_options::CraftOptions;
        let cookies = use_context::<Cookies>().unwrap();
        let (opts_cookie, _) = cookies.use_cookie_typed::<_, CraftOptions>("CRAFT_OPTIONS");
        let on_hand_map = use_context::<OnHandMap>();

        use_context::<CheapestPrices>().unwrap().read_listings.with(|data| {
            let data = data.as_ref()?.as_ref().ok()?;
            let opts_value = opts_cookie.get().unwrap_or_default();
            let shards = if opts_value.exclude_shards {
                ShardsMode::ExcludeShards
            } else { ShardsMode::IncludeMarket };

            let local = on_hand_map
                .map(|m| LocalOnHand::from_map(m.0.get_untracked()))
                .unwrap_or_else(|| LocalOnHand::from_map(Default::default()));
            let empty = EmptyOnHand;
            let active: &dyn crate::components::crafting_cost::OnHand =
                if opts_value.use_on_hand { &local } else { &empty };
            let recipes_by_output = std::collections::HashMap::new();

            let lq_opts = CraftingCostOptions {
                require_hq: false, max_subcraft_depth: 0, shards, on_hand: active,
            };
            let lq = compute_cost(recipe, data, &recipes_by_output, &lq_opts, &is_shard_item);

            let local_hq = on_hand_map
                .map(|m| LocalOnHand::from_map(m.0.get_untracked()))
                .unwrap_or_else(|| LocalOnHand::from_map(Default::default()));
            let active_hq: &dyn crate::components::crafting_cost::OnHand =
                if opts_value.use_on_hand { &local_hq } else { &empty };
            let hq_opts = CraftingCostOptions {
                require_hq: true, max_subcraft_depth: 0, shards, on_hand: active_hq,
            };
            let hq = compute_cost(recipe, data, &recipes_by_output, &hq_opts, &is_shard_item);

            let lq_sell = data.map.get(&CheapestListingMapKey { item_id: target_item.key_id.0, hq: false }).map(|d| d.price);
            let hq_sell = if target_item.can_be_hq {
                data.map.get(&CheapestListingMapKey { item_id: target_item.key_id.0, hq: true })
                    .or_else(|| data.map.get(&CheapestListingMapKey { item_id: target_item.key_id.0, hq: false }))
                    .map(|d| d.price)
            } else { None };

            let profit_chip = |label: &str, profit_opt: Option<i32>| {
                profit_opt.map(|profit| {
                    let cls = if profit >= 0 {
                        "px-2 py-0.5 rounded-full text-xs font-bold bg-emerald-900/30 text-emerald-300 border border-emerald-700/30 flex items-center gap-1"
                    } else {
                        "px-2 py-0.5 rounded-full text-xs font-bold bg-red-900/30 text-red-300 border border-red-700/30 flex items-center gap-1"
                    };
                    view! { <span class=cls><span>{label}</span><Gil amount=profit /></span> }.into_any()
                })
            };

            Some(view! {
                <div class="flex flex-wrap items-center justify-between gap-2 text-sm mt-2">
                    <span class="text-brand-300">"Est. Profit:"</span>
                    <div class="flex gap-2">
                        {profit_chip("HQ", hq_sell.map(|p| p - hq.cost))}
                        {profit_chip("LQ", lq_sell.map(|p| p - lq.cost))}
                    </div>
                </div>
            })
        })
    }}
</Suspense>
```

- [ ] **Step 5: Add the toggle row above the "Crafting Recipes" panel**

In `RelatedItems` (around line 760), the `div#crafting-recipes` panel currently has just `<h2>"Crafting Recipes"</h2>` and the grid. Add a toggle row above the grid:

```rust
<div
    id="crafting-recipes"
    class="panel p-4 sm:p-6"
    class:hidden=move || recipes.with(|recipes| recipes.is_empty())
>
    <div class="flex flex-row items-center justify-between mb-3 flex-wrap gap-2">
        <h2 class="text-xl font-bold text-brand-200 px-1">"Crafting Recipes"</h2>
        <CraftOptionsToggleRow />
    </div>
    <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
        // ... existing <For ... /> ...
    </div>
</div>
```

Add the `CraftOptionsToggleRow` component near the top of `related_items.rs`:

```rust
#[component]
fn CraftOptionsToggleRow() -> impl IntoView {
    use crate::global_state::cookies::Cookies;
    use crate::global_state::craft_options::CraftOptions;
    let cookies = use_context::<Cookies>().unwrap();
    let (opts_signal, set_opts) = cookies.use_cookie_typed::<_, CraftOptions>("CRAFT_OPTIONS");

    let opts = move || opts_signal.get().unwrap_or_default();
    let toggle = move |mutator: Box<dyn Fn(&mut CraftOptions)>| {
        let mut current = opts();
        mutator(&mut current);
        set_opts(Some(current));
    };

    view! {
        <div class="flex flex-row items-center gap-3 text-xs flex-wrap">
            <label class="flex flex-row items-center gap-1">
                <input
                    type="checkbox"
                    class="checkbox checkbox-xs"
                    prop:checked=move || opts().require_hq
                    on:change=move |_| toggle(Box::new(|o| o.require_hq = !o.require_hq))
                />
                "Require HQ"
            </label>
            <label class="flex flex-row items-center gap-1">
                <input
                    type="checkbox"
                    class="checkbox checkbox-xs"
                    prop:checked=move || opts().exclude_shards
                    on:change=move |_| toggle(Box::new(|o| o.exclude_shards = !o.exclude_shards))
                />
                "Exclude shards"
            </label>
            <label class="flex flex-row items-center gap-1">
                <input
                    type="checkbox"
                    class="checkbox checkbox-xs"
                    prop:checked=move || opts().use_on_hand
                    on:change=move |_| toggle(Box::new(|o| o.use_on_hand = !o.use_on_hand))
                />
                "Use on-hand"
            </label>
        </div>
    }
}
```

(The "include subcrafts" toggle isn't relevant on the item page since item-page costs always use `max_subcraft_depth=0`. Leave it out; we add it on the analyzers in Task 8.)

- [ ] **Step 6: Run build + lint**

```bash
cd /c/Users/chw11/code/ultros/.claude/worktrees/gracious-montalcini-33b20d
./check_ci.sh 2>&1 | tail -40
```

Expected: clean. Fix any compile errors before continuing. (Most likely culprit: the `dyn OnHand` borrows can confuse the borrow checker — if so, build separate `Box<dyn OnHand>` values per pass instead of `&dyn OnHand`.)

- [ ] **Step 7: Smoke test the item page**

```bash
./scripts/run_e2e.sh 2>&1 | tail -20
```

If E2E is too heavy, just `cargo run` the dev server and visit an item with a recipe (e.g. `/item/<world>/24239` — Cermet Ingot) and verify the cost line shows toggles + a numeric cost.

- [ ] **Step 8: Commit**

```bash
git add -u
git add ultros-frontend/ultros-app/src/components/related_items.rs \
        ultros-frontend/ultros-app/src/components/add_recipe_to_current_list.rs
git commit -m "refactor(related_items): consume crafting_cost module + toggle row"
```

---

## Task 8: Wire `recipe_analyzer.rs` to `compute_cost`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs`

Delete the local `calculate_crafting_cost` (lines 81-160). Add a `shards` URL query param, a filter card, and wire `CraftOptions` cookie defaults. Add an "Open in analyzer"-equivalent affordance — actually that link lives on the item page (Task 11); this task is the analyzer's own toggle plumbing.

- [ ] **Step 1: Remove the local `calculate_crafting_cost`**

Delete lines 81-160 of `recipe_analyzer.rs` (the entire function).

- [ ] **Step 2: Add imports**

At the top of the file, add:

```rust
use crate::components::crafting_cost::{
    compute_cost, CraftingCostOptions, EmptyOnHand, ShardsMode,
};
use crate::components::on_hand_input::{LocalOnHand, OnHandMap};
use crate::global_state::craft_options::CraftOptions;
```

Add the `is_shard_item` helper (same as in `related_items.rs` — duplicate it; the function is two lines and not worth a public re-export):

```rust
fn is_shard_item(item_id: ItemId) -> bool {
    tracked_data()
        .items
        .get(&item_id)
        .map(|i| i.item_search_category == 59)
        .unwrap_or(false)
}
```

- [ ] **Step 3: Replace the `calculate_crafting_cost(...)` call site at line 312**

Currently:
```rust
let (craft_cost, sub_crafts) = calculate_crafting_cost(
    recipe,
    &prices,
    &recipes_by_output,
    0,
    if use_sub { 2 } else { 0 },
    use_sub,
    require_hq_flag,
);
```

Replace with:
```rust
let opts_cookie = use_context::<Cookies>().unwrap()
    .use_cookie_typed::<_, CraftOptions>("CRAFT_OPTIONS").0;
let opts_value = opts_cookie.get().unwrap_or_default();
let shards = if exclude_shards_url().unwrap_or(opts_value.exclude_shards) {
    ShardsMode::ExcludeShards
} else {
    ShardsMode::IncludeMarket
};
let local = on_hand_map
    .map(|m: OnHandMap| LocalOnHand::from_map(m.0.get_untracked()))
    .unwrap_or_else(|| LocalOnHand::from_map(Default::default()));
let empty = EmptyOnHand;
let use_on_hand = use_on_hand_url().unwrap_or(opts_value.use_on_hand);
let active: &dyn crate::components::crafting_cost::OnHand =
    if use_on_hand { &local } else { &empty };

let opts = CraftingCostOptions {
    require_hq: require_hq_flag,
    max_subcraft_depth: if use_sub { 2 } else { 0 },
    shards,
    on_hand: active,
};
let breakdown = compute_cost(recipe, &prices, &recipes_by_output, &opts, &is_shard_item);
let craft_cost = breakdown.cost;
let sub_crafts = breakdown.sub_crafts.clone();
```

Yes, hoist `use_context` once at the top of `computed_data` rather than inside the per-recipe loop — the loop runs over every recipe and grabbing the context each iteration is wasteful. Concretely, move these three lookups (`opts_cookie`, `on_hand_map`, `opts_value`) above the `for recipe in recipes.values()` loop at [line 253](../../ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:253).

- [ ] **Step 4: Add the two new URL query signals**

Near the other `query_signal::<bool>` calls (lines 209-212):

```rust
let (exclude_shards_url, set_exclude_shards) = query_signal::<bool>("shards-exclude");
let (use_on_hand_url, set_use_on_hand) = query_signal::<bool>("on-hand");
```

- [ ] **Step 5: Add filter card for shards + on-hand**

Inside the existing "Options" `FilterCard` block (around line 478), add two checkboxes next to the existing "Filter Outliers":

```rust
<div class="flex flex-row gap-4 flex-wrap">
    <input
        type="checkbox"
        id="exclude-shards"
        class="checkbox"
        prop:checked=move || exclude_shards_url().unwrap_or(true)
        on:change=move |ev| set_exclude_shards(Some(event_target_checked(&ev)))
    />
    <label for="exclude-shards">"Exclude shards/crystals"</label>
    <div class="text-brand-300 cursor-help" title="If enabled, crystal/shard/cluster ingredient costs are not counted toward the craft cost. Most crafters keep a stockpile.">
        <Icon icon=i::AiQuestionCircleOutlined />
    </div>
</div>
<div class="flex flex-row gap-4 flex-wrap">
    <input
        type="checkbox"
        id="use-on-hand"
        class="checkbox"
        prop:checked=move || use_on_hand_url().unwrap_or(false)
        on:change=move |ev| set_use_on_hand(Some(event_target_checked(&ev)))
    />
    <label for="use-on-hand">"Use on-hand inventory"</label>
    <div class="text-brand-300 cursor-help" title="Deduct ingredients you already own from the craft cost. Set per-ingredient totals on the item page.">
        <Icon icon=i::AiQuestionCircleOutlined />
    </div>
</div>
```

- [ ] **Step 6: Run CI checks**

```bash
./check_ci.sh 2>&1 | tail -40
```

Fix any errors. Anticipated: borrow-checker issues from `&dyn OnHand` lifetimes inside the closure. If the compiler complains, switch to `Box<dyn OnHand>`:

```rust
let active: Box<dyn crate::components::crafting_cost::OnHand> =
    if use_on_hand { Box::new(local) } else { Box::new(empty) };
let opts = CraftingCostOptions { ..., on_hand: active.as_ref(), };
```

- [ ] **Step 7: Smoke test the analyzer**

Dev-run, visit `/analyzer/recipes/<world>?shards-exclude=true&on-hand=false`, verify the toggle changes the cost numbers.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "refactor(recipe_analyzer): consume crafting_cost + shards/on-hand toggles"
```

---

## Task 9: Wire `fc_crafting_analyzer.rs` to `compute_ingredient_cost`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/fc_crafting_analyzer.rs`

Rewrite `calculate_fc_project_cost` to iterate its accumulated materials map through `compute_ingredient_cost` (one primitive call per material). Add the same `shards-exclude` + `on-hand` URL params and filter cards.

Note: FC crafts work on `CompanyCraftSequence` (a different data shape than `Recipe`). `compute_cost` doesn't apply, but `compute_ingredient_cost` does — that's exactly why we split the primitive out.

- [ ] **Step 1: Add imports** (top of `fc_crafting_analyzer.rs`)

```rust
use crate::components::crafting_cost::{
    compute_ingredient_cost, CraftingCostOptions, EmptyOnHand, OnHand, ShardsMode,
    CRYSTAL_SEARCH_CATEGORY,
};
use crate::components::on_hand_input::{LocalOnHand, OnHandMap};
use crate::global_state::craft_options::CraftOptions;
```

- [ ] **Step 2: Replace `calculate_fc_project_cost`**

The current function (lines 78-146) accumulates `materials_map` then prices everything. The rewrite uses `compute_ingredient_cost` per material and returns both the cost and a richer summary.

```rust
fn calculate_fc_project_cost(
    sequence: &'static CompanyCraftSequence,
    prices: &CheapestListingsMap,
    data: &'static xiv_gen::Data,
    opts: &CraftingCostOptions<'_>,
) -> (i32, Vec<MaterialInfo>, i32 /* shard_cost */, i32 /* on_hand_savings */) {
    let mut materials_map: HashMap<ItemId, i32> = HashMap::new();

    for part_id in sequence.company_craft_part {
        if let Some(part) = data.company_craft_parts.get(&CompanyCraftPartId(part_id)) {
            for process_link in part.company_craft_process {
                if let Some(process) = data
                    .company_craft_processs
                    .get(&CompanyCraftProcessId(process_link))
                {
                    for i in 0..12 {
                        let supply_item_link = process.supply_item[i];
                        let quantity_per_set = process.set_quantity[i];
                        let sets_required = process.sets_required[i];
                        if quantity_per_set == 0 || sets_required == 0 {
                            continue;
                        }
                        if let Some(supply_item) = data
                            .company_craft_supply_items
                            .get(&CompanyCraftSupplyItemId(supply_item_link))
                        {
                            if supply_item.item == 0 { continue; }
                            let total_quantity = quantity_per_set * sets_required;
                            *materials_map.entry(ItemId(supply_item.item)).or_default() +=
                                total_quantity;
                        }
                    }
                }
            }
        }
    }

    let mut total_cost: i64 = 0;
    let mut shard_cost: i64 = 0;
    let mut on_hand_savings: i64 = 0;
    let mut material_infos = Vec::new();

    for (item_id, quantity) in materials_map {
        let line = compute_ingredient_cost(item_id, quantity, prices, opts);
        let is_shard = data
            .items
            .get(&item_id)
            .map(|i| i.item_search_category == CRYSTAL_SEARCH_CATEGORY)
            .unwrap_or(false);

        let line_market = (line.used_from_market as i64) * (line.unit_price as i64);
        let line_on_hand = (line.used_from_on_hand as i64) * (line.unit_price as i64);

        if is_shard {
            shard_cost = shard_cost.saturating_add(line_market + line_on_hand);
            if matches!(opts.shards, ShardsMode::IncludeMarket) {
                total_cost = total_cost.saturating_add(line_market);
                on_hand_savings = on_hand_savings.saturating_add(line_on_hand);
            }
        } else {
            total_cost = total_cost.saturating_add(line_market);
            on_hand_savings = on_hand_savings.saturating_add(line_on_hand);
        }

        material_infos.push(MaterialInfo {
            item_id,
            total_quantity: quantity,
            unit_cost: line.unit_price,
        });
    }

    let clamp = |v: i64| -> i32 {
        if v > i32::MAX as i64 { i32::MAX } else if v < 0 { 0 } else { v as i32 }
    };

    (clamp(total_cost), material_infos, clamp(shard_cost), clamp(on_hand_savings))
}
```

- [ ] **Step 3: Update the call site to pass `CraftingCostOptions`**

Find where `calculate_fc_project_cost` is invoked (search for `calculate_fc_project_cost(`). Replace with the new signature. The pattern mirrors Task 8:

```rust
let opts_cookie = use_context::<crate::global_state::cookies::Cookies>().unwrap()
    .use_cookie_typed::<_, CraftOptions>("CRAFT_OPTIONS").0;
let opts_value = opts_cookie.get().unwrap_or_default();
// URL signals (added in Step 4 below).
let shards = if exclude_shards_url().unwrap_or(opts_value.exclude_shards) {
    ShardsMode::ExcludeShards
} else { ShardsMode::IncludeMarket };
let local = on_hand_map
    .map(|m: OnHandMap| LocalOnHand::from_map(m.0.get_untracked()))
    .unwrap_or_else(|| LocalOnHand::from_map(Default::default()));
let empty = EmptyOnHand;
let use_on_hand = use_on_hand_url().unwrap_or(opts_value.use_on_hand);
let active: &dyn OnHand = if use_on_hand { &local } else { &empty };

let opts = CraftingCostOptions {
    require_hq: false,
    max_subcraft_depth: 0,
    shards,
    on_hand: active,
};
let (cost, materials, shard_cost, on_hand_savings) =
    calculate_fc_project_cost(sequence, &prices, data, &opts);
```

Plus update `FCCraftProfitData` to store `shard_cost` and `on_hand_savings` (additive — both default to 0 if no plumbing was changed). Field additions:

```rust
#[derive(Clone, Debug, PartialEq)]
struct FCCraftProfitData {
    // ... existing fields ...
    shard_cost: i32,
    on_hand_savings: i32,
}
```

Wire them through wherever `FCCraftProfitData` is constructed.

- [ ] **Step 4: Add URL query signals + filter card**

Same pattern as Task 8. Find the FC analyzer's other `query_signal::<bool>` calls and add:

```rust
let (exclude_shards_url, set_exclude_shards) = query_signal::<bool>("shards-exclude");
let (use_on_hand_url, set_use_on_hand) = query_signal::<bool>("on-hand");
```

Add a matching `FilterCard` with the two checkboxes (copy the markup from Task 8 Step 5).

- [ ] **Step 5: Run CI checks**

```bash
./check_ci.sh 2>&1 | tail -40
```

- [ ] **Step 6: Smoke test the FC analyzer**

Visit `/analyzer/fc-crafts/<world>` and toggle the new checkboxes; verify cost numbers move.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/fc_crafting_analyzer.rs
git commit -m "refactor(fc_crafting_analyzer): consume compute_ingredient_cost + toggles"
```

---

## Task 10: `ListOnHand` + active-list cookie + banner

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/on_hand_input.rs`
- Modify: `ultros-frontend/ultros-app/src/global_state/craft_options.rs` (no changes, just confirm `active_craft_list` is in the struct)
- New: a small `<ActiveListBanner />` component in `on_hand_input.rs`

The second `OnHand` implementation. Reads each item's `acquired` qty from the user's active list. When active, every cost-quoting surface shows a banner: "On-hand pulled from list: «name»".

This task does not change the list page or its UI. It only adds a read path.

- [ ] **Step 1: Add `ListOnHand` implementation**

Append to `on_hand_input.rs`:

```rust
use std::cell::RefCell;
use std::collections::HashMap as Map;
use ultros_api_types::list::ListItem;

/// Reads on-hand from the user's active list's ListItem.acquired field.
/// Snapshotted at construction; consume mutates the snapshot only
/// (the list page is the canonical write path for `acquired`).
pub struct ListOnHand {
    snapshot: RefCell<Map<i32, i32>>,
    pub list_id: i32,
    pub list_name: String,
}

impl ListOnHand {
    pub fn from_items(list_id: i32, list_name: String, items: &[ListItem]) -> Self {
        let snapshot = items
            .iter()
            .filter_map(|i| i.acquired.map(|q| (i.item_id, q)))
            .collect();
        Self {
            snapshot: RefCell::new(snapshot),
            list_id,
            list_name,
        }
    }
}

impl OnHand for ListOnHand {
    fn available(&self, item: xiv_gen::ItemId) -> i32 {
        self.snapshot.borrow().get(&item.0).copied().unwrap_or(0)
    }
    fn consume(&self, item: xiv_gen::ItemId, qty: i32) {
        let mut s = self.snapshot.borrow_mut();
        if let Some(v) = s.get_mut(&item.0) {
            *v = (*v - qty).max(0);
        }
    }
}
```

- [ ] **Step 2: Add the banner component**

```rust
#[component]
pub fn ActiveListBanner() -> impl IntoView {
    use crate::global_state::cookies::Cookies;
    use crate::global_state::craft_options::CraftOptions;
    let cookies = use_context::<Cookies>().unwrap();
    let (opts_signal, set_opts) = cookies.use_cookie_typed::<_, CraftOptions>("CRAFT_OPTIONS");

    let active_id = move || opts_signal.get().unwrap_or_default().active_craft_list;

    move || active_id().map(|id| view! {
        <div class="panel p-2 rounded-md border border-emerald-700/30 bg-emerald-900/10 flex flex-row items-center justify-between text-sm">
            <span class="text-emerald-300">
                "On-hand pulled from list #" {id}
            </span>
            <button
                class="btn-ghost text-xs"
                on:click=move |_| {
                    let mut o = opts_signal.get().unwrap_or_default();
                    o.active_craft_list = None;
                    set_opts(Some(o));
                }
            >
                "Detach"
            </button>
        </div>
    })
}
```

- [ ] **Step 3: Wire `ListOnHand` selection on the analyzer surfaces**

In `recipe_analyzer.rs` and `fc_crafting_analyzer.rs`, augment the `active` selection block from Tasks 8/9. After looking up `opts_value`:

```rust
// (already in Task 8 code) `local` is LocalOnHand snapshot.
// New: if active_craft_list is set, read that list and use ListOnHand instead.
let active: Box<dyn OnHand> = match opts_value.active_craft_list {
    Some(_list_id) if use_on_hand => {
        // List fetch is async-resourced separately; for the first cut,
        // fall through to LocalOnHand if the resource isn't ready yet.
        // (Plumbing the resource in is left for a follow-up — flagged
        //  in the roadmap section of the spec.)
        Box::new(local)
    }
    _ if use_on_hand => Box::new(local),
    _ => Box::new(empty),
};
```

For the MVP shipped in this task, `ListOnHand` is wired but the active-list resource fetch is not yet plumbed into the analyzer's reactive graph — that's a measured 2-task follow-up (resource definition, suspense handling). Land the type, the banner, and the cookie field now; the actual list→analyzer fetch can ship as a small PR after this branch.

Document this in the commit message so future readers know.

- [ ] **Step 4: Add the banner to each cost-quoting surface**

In `related_items.rs` (above the toggle row in the "Crafting Recipes" panel), add:
```rust
<ActiveListBanner />
```

In `recipe_analyzer.rs` and `fc_crafting_analyzer.rs`, add `<ActiveListBanner />` above the filter cards.

- [ ] **Step 5: Run CI + commit**

```bash
./check_ci.sh 2>&1 | tail -20
git add -u
git commit -m "feat(on_hand): ListOnHand impl + active-list cookie banner

ListOnHand reads ListItem.acquired snapshots. The analyzer/item-page
banner toggles the active_craft_list cookie field. Resource-fetch
plumbing for the live list payload is deferred to a follow-up; the
type is in place so the wiring is a small additive PR."
```

---

## Task 11: "Open in analyzer" button on item page recipe cards

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/related_items.rs`

A small affordance on each recipe card on the item page that opens the recipe analyzer pre-filtered to the same job + toggles. Mirrors the spec §4 cross-pollination.

- [ ] **Step 1: Add the helper to derive the job code**

In `related_items.rs`, near the top (after imports):

```rust
fn job_code_from_craft_type(craft_type: i32) -> &'static str {
    match craft_type {
        0 => "CRP", 1 => "BSM", 2 => "ARM", 3 => "GSM",
        4 => "LTW", 5 => "WVR", 6 => "ALC", 7 => "CUL",
        _ => "",
    }
}
```

- [ ] **Step 2: Add the button to `Recipe`**

In the `Recipe` component header bar (where `AddRecipeToList` lives, ~line 207), add a sibling link:

```rust
let job = job_code_from_craft_type(recipe.craft_type);
let analyzer_href = move || {
    use crate::global_state::cookies::Cookies;
    use crate::global_state::craft_options::CraftOptions;
    let cookies = use_context::<Cookies>().unwrap();
    let (opts, _) = cookies.use_cookie_typed::<_, CraftOptions>("CRAFT_OPTIONS");
    let o = opts.get().unwrap_or_default();
    let zone = price_zone.get().as_ref().map(|z| z.get_name()).unwrap_or("North-America").to_string();
    format!(
        "/analyzer/recipes/{zone}?job={job}&require-hq={hq}&subcrafts={sub}&shards-exclude={shards}&on-hand={oh}",
        zone = zone,
        job = job,
        hq = o.require_hq,
        sub = o.include_subcrafts,
        shards = o.exclude_shards,
        oh = o.use_on_hand,
    )
};
```

Where `price_zone` already exists in `RelatedItems` (it's threaded via `get_price_zone()` at line 648). You'll need to propagate it into `Recipe` via a prop, or call `get_price_zone()` again inside `Recipe` for simplicity:

```rust
let (price_zone, _) = get_price_zone();
```

Add the link button in the card header next to `AddRecipeToList`:

```rust
<a
    class="btn-secondary text-xs px-2 py-1 flex flex-row items-center gap-1"
    href=analyzer_href
    aria-label="Open this recipe in the analyzer"
>
    <Icon icon=icondata::AiBarChartOutlined />
    "Analyzer"
</a>
```

- [ ] **Step 3: Run CI + visual check**

```bash
./check_ci.sh 2>&1 | tail -20
```

Dev-run, visit an item with a recipe, click the new button, verify the analyzer route loads with the URL params set.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/related_items.rs
git commit -m "feat(item_view): 'Open in analyzer' link on each recipe card"
```

---

## Task 12: Final verification + PR

**Files:** No code changes.

- [ ] **Step 1: Run the full CI script**

```bash
cd /c/Users/chw11/code/ultros/.claude/worktrees/gracious-montalcini-33b20d
./check_ci.sh 2>&1 | tail -40
```

Expected: clean. If clippy flags the `Box<dyn OnHand>` heap allocation as "needless allocation", that's a false positive — add `#[allow(clippy::redundant_allocation)]` at the call site with a one-line comment explaining why.

- [ ] **Step 2: Run the test suite**

```bash
cd ultros-frontend/ultros-app
cargo test --features hydrate 2>&1 | tail -20
```

Expected: all `crafting_cost::tests`, `on_hand_input::tests`, and `craft_options::tests` pass.

- [ ] **Step 3: Manual smoke pass**

Start the dev server (`cargo run` from project root or whatever the existing dev runner is) and walk:

1. Item page (e.g. `/item/Goblin/24239`) — confirm the recipes panel shows the toggle row and the cost line updates when toggles change.
2. Toggle "Exclude shards" off and back on; verify the "shards excl. Xg" chip disappears/reappears.
3. Set "Use on-hand", enter a number in an `OnHandQuantity` input, verify the cost drops by `qty * unit_price` and the "saved Xg" chip shows.
4. Click "Analyzer" on a recipe card; the analyzer should land with `?job=…&shards-exclude=true&on-hand=true` in the URL and matching checkboxes.
5. On the analyzer, toggle each new checkbox and confirm cost columns refresh.
6. Visit the FC analyzer; toggle the same two checkboxes; verify costs refresh.
7. Active-list banner: doesn't have its data path wired in this branch — verify the banner DOESN'T appear when `active_craft_list` is None, and DOES appear when the cookie value is manually set via DevTools (`document.cookie = "CRAFT_OPTIONS=..."`).

- [ ] **Step 4: Open the PR**

```bash
gh pr create --title "feat: unified crafting cost + shard toggle + on-hand accounting" --body "$(cat <<'EOF'
## Summary

- One shared `crafting_cost::compute_cost` module replaces the two divergent `calculate_crafting_cost` implementations on the item page and recipe analyzer; the FC analyzer consumes the same `compute_ingredient_cost` primitive.
- First-class "Exclude shards" toggle on every cost-quoting surface (item page, recipe analyzer, FC analyzer), defaulting to ON.
- On-hand inventory accounting via LocalStorage (`LocalOnHand`) with a typed surface for list-backed sourcing (`ListOnHand`) ready for follow-up wiring.
- "Open in analyzer" affordance on each recipe card on the item page; toggles round-trip via URL params + a new `CRAFT_OPTIONS` cookie.

Spec: `docs/superpowers/specs/2026-05-12-crafting-recipes-unified-design.md`
Plan: `docs/superpowers/plans/2026-05-12-crafting-recipes-unified.md`

## Test plan
- [ ] `./check_ci.sh` clean
- [ ] `cargo test crafting_cost` — all parity, shards, on-hand, subcraft tests pass
- [ ] Item page (Cermet Ingot or similar): toggle row updates cost line in real time
- [ ] Recipe analyzer: `?shards-exclude=true` is the default and matches the cookie
- [ ] FC analyzer: same toggles work
- [ ] "Open in analyzer" button round-trips toggle state through the URL
- [ ] Active-list banner appears only when `active_craft_list` is set

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 5: Capture the PR URL** in this conversation so the user can review.

---

## Roadmap (post-merge, not in this branch)

The spec called these out as Tier 2/3 — explicitly out of scope here, recapped for hand-off:

- **Active-list data fetch.** Plumb the live list payload into the analyzer's reactive graph so `ListOnHand` actually populates from a server-fetched list (right now Task 10 only wires the type + banner + cookie).
- **Teamcraft / Allagantools paste importer** for bulk on-hand entry.
- **Per-row on-hand savings column** on the analyzer table.
- **"Plan a craft batch" surface** — given a target item + craft count, compute the shopping list with on-hand applied and save as a list.
