//! The compact cart's per-row estimate: the cost of the units the player
//! still has to buy, filled from the cheapest listings the page already
//! holds for the row. The arithmetic lives in `ultros_calc::list_estimate`
//! (#1431), the same engine that totals the cart above the rows, so a row
//! and the summary never disagree; this only adapts a document row to it.

use ultros_api_types::{ActiveListing, list::ListItem};
use ultros_calc::list_estimate::LineRequest;

pub use ultros_calc::list_estimate::{LineEstimate, LineStatus};

/// Estimate one row from the listings the page holds for its item.
pub fn estimate_line(item: &ListItem, listings: &[ActiveListing]) -> LineEstimate {
    ultros_calc::list_estimate::estimate_line(LineRequest::from(item), listings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(quantity: i32, acquired: i32, hq: Option<bool>) -> ListItem {
        ListItem {
            id: 7,
            item_id: 1,
            list_id: 1,
            hq,
            quantity: Some(quantity),
            acquired: Some(acquired),
            target_price: None,
        }
    }

    fn listing(id: i32, price: i32, quantity: i32, hq: bool) -> ActiveListing {
        ActiveListing {
            id,
            world_id: 1,
            item_id: 1,
            retainer_id: 1,
            price_per_unit: price,
            quantity,
            hq,
            timestamp: chrono::NaiveDateTime::default(),
        }
    }

    /// The row adapter carries the document row's identity, need and owned
    /// units into the shared engine; the engine's own tests own the rest.
    #[test]
    fn a_row_prices_its_remaining_units_cheapest_first() {
        let listings = [listing(1, 30, 5, false), listing(2, 10, 2, false)];
        let line = estimate_line(&item(5, 1, None), &listings);
        assert_eq!(line.row_id, 7);
        assert_eq!(line.remaining, 4);
        assert_eq!(line.total, 2 * 10 + 2 * 30);
        assert_eq!(line.unit_price, Some(10));
        assert_eq!(line.status, LineStatus::Priced);
        let hq = estimate_line(&item(2, 0, Some(true)), &listings);
        assert_eq!(hq.status, LineStatus::NoSupply);
        assert_eq!(hq.unit_price, None);
        assert_eq!(
            estimate_line(&item(3, 3, None), &listings).status,
            LineStatus::Acquired
        );
    }
}
