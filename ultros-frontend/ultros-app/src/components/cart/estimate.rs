//! The compact cart consumes one allocation for its entire source, then
//! shares those exact line results with costs, details and sorting.

use std::collections::HashMap;
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
    id: i64,
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

/// Units acquired against units needed across the whole cart, before any
/// filter. `None` for an empty cart, so the summary shows no progress line
/// rather than "0 / 0". Over-acquired rows count as complete, never as more.
pub fn progress_units(
    rows: &[(ultros_api_types::list::ListItem, Vec<ActiveListing>)],
) -> Option<(i32, i32)> {
    let mut needed: i32 = 0;
    let mut acquired: i32 = 0;
    for (item, _) in rows {
        let quantity = item.quantity.unwrap_or(1).max(1);
        needed = needed.saturating_add(quantity);
        acquired = acquired.saturating_add(item.acquired.unwrap_or(0).clamp(0, quantity));
    }
    (needed > 0).then_some((acquired, needed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::list::ListItem;

    fn row(quantity: Option<i32>, acquired: Option<i32>) -> (ListItem, Vec<ActiveListing>) {
        (
            ListItem {
                quantity,
                acquired,
                ..Default::default()
            },
            Vec::new(),
        )
    }

    #[test]
    fn progress_is_none_for_an_empty_cart() {
        assert_eq!(progress_units(&[]), None);
    }

    #[test]
    fn progress_clamps_acquired_and_defaults_quantity_to_one() {
        let rows = [
            row(Some(4), Some(2)),
            row(None, Some(9)),
            row(Some(3), None),
        ];
        assert_eq!(progress_units(&rows), Some((3, 8)));
    }
}
