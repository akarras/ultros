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

/// A recipe that takes 1 ingredient (no shards). Item 2000 is reserved
/// for Task 4's subcraft tests (as a craftable intermediate).
pub fn fixture_simple_recipe_prices() -> CheapestListingsMap {
    CheapestListingsMap::from(CheapestListings {
        cheapest_listings: vec![
            CheapestListingItem {
                item_id: 1000,
                hq: false,
                world_id: 1,
                cheapest_price: 100,
            },
            CheapestListingItem {
                item_id: 2000,
                hq: false,
                world_id: 1,
                cheapest_price: 50,
            },
        ],
    })
}

/// Prices for a recipe that mixes shard and non-shard ingredients.
/// Ingredient 1000 = non-shard @ 100g, ingredient 1001 = shard @ 5g.
pub fn fixture_shard_recipe_prices() -> CheapestListingsMap {
    CheapestListingsMap::from(CheapestListings {
        cheapest_listings: vec![
            CheapestListingItem {
                item_id: 1000,
                hq: false,
                world_id: 1,
                cheapest_price: 100,
            },
            CheapestListingItem {
                item_id: 1001,
                hq: false,
                world_id: 1,
                cheapest_price: 5,
            },
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
