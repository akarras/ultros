//! The compact cart consumes one allocation for its entire source, then
//! shares those exact line results with costs, details and sorting.

use std::collections::HashMap;
#[cfg(test)]
use ultros_api_types::ActiveListing;
use ultros_calc::list_estimate::CartEstimate;

pub use ultros_calc::list_estimate::{LineEstimate, LineStatus};

pub fn lines_by_id(estimate: &CartEstimate) -> HashMap<i32, LineEstimate> {
    estimate
        .lines
        .iter()
        .map(|line| (line.row_id, line.clone()))
        .collect()
}

/// Test fixture: `quantity` units of `item_id` at `price` gil per unit.
#[cfg(test)]
pub fn fixture_listing(
    id: i32,
    item_id: i32,
    price: i32,
    quantity: i32,
    hq: bool,
) -> ActiveListing {
    ActiveListing {
        id,
        world_id: 1,
        item_id,
        retainer_id: 1,
        price_per_unit: price,
        quantity,
        hq,
        timestamp: chrono::NaiveDateTime::default(),
    }
}
