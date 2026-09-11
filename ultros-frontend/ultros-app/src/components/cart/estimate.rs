//! Build-cart estimate contract (Track B's stand-in until #1431 lands).
//!
//! A line estimates the cost of the units the player still has to buy
//! (needed minus owned) by filling that need from the cheapest observed
//! listings whose quality matches the row. Supply is what the page already
//! fetched for the row, so nothing here reaches the network. The result
//! says how many units that supply covers: a short line's total is a lower
//! bound, never a guaranteed basket. Shop's whole-stack costs stay separate.

use ultros_api_types::{ActiveListing, list::ListItem};

use crate::routes::list_view::remaining_quantity;

/// How much of a line's remaining need the observed listings can supply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// Every remaining unit has a listing behind it (including zero need).
    Full,
    /// Some units are covered; the total only prices those.
    Partial,
    /// No matching listing at all; the total is unknown.
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineEstimate {
    /// Units still to buy: needed minus owned, floored at zero.
    pub requested: i32,
    /// Units the observed listings can supply, at most `requested`.
    pub covered: i32,
    /// Gil for the covered units. `None` when nothing matched and units
    /// were requested; `Some(0)` when nothing is left to buy.
    pub total: Option<i64>,
    /// Cheapest matching unit price, when any listing matched.
    pub unit_price: Option<i32>,
    pub coverage: Coverage,
}

/// Aggregate over the rows the cart currently shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CartEstimate {
    /// Gil for every covered unit across the cart (saturating).
    pub total: i64,
    pub lines: usize,
    /// Lines with some but not all units covered.
    pub short_lines: usize,
    /// Lines with units requested but no listing at all.
    pub unknown_lines: usize,
    pub requested_units: i64,
    pub covered_units: i64,
}

impl CartEstimate {
    /// True when the total prices every requested unit.
    pub fn complete(&self) -> bool {
        self.short_lines == 0 && self.unknown_lines == 0
    }
}

/// NQ rows take NQ listings, HQ rows HQ, and an "Any" row takes both.
pub fn matches_quality(item: &ListItem, listing: &ActiveListing) -> bool {
    item.hq.is_none_or(|hq| listing.hq == hq)
}

/// Matching listings, cheapest first, then largest stack, then id.
pub fn matching_listings<'a>(
    item: &ListItem,
    listings: &'a [ActiveListing],
) -> Vec<&'a ActiveListing> {
    let mut matching: Vec<&ActiveListing> = listings
        .iter()
        .filter(|listing| matches_quality(item, listing))
        .collect();
    matching.sort_by_key(|listing| {
        (
            listing.price_per_unit,
            std::cmp::Reverse(listing.quantity),
            listing.id,
        )
    });
    matching
}

pub fn estimate_line(item: &ListItem, listings: &[ActiveListing]) -> LineEstimate {
    let requested = remaining_quantity(item).max(0);
    let matching = matching_listings(item, listings);
    let unit_price = matching.first().map(|listing| listing.price_per_unit);
    let mut covered: i32 = 0;
    let mut total: i64 = 0;
    for listing in &matching {
        let need = requested - covered;
        if need <= 0 {
            break;
        }
        let take = listing.quantity.clamp(0, need);
        if take == 0 {
            continue;
        }
        let cost = i64::from(take)
            .checked_mul(i64::from(listing.price_per_unit.max(0)))
            .unwrap_or(i64::MAX);
        total = total.saturating_add(cost);
        covered += take;
    }
    let coverage = if covered >= requested {
        Coverage::Full
    } else if covered > 0 {
        Coverage::Partial
    } else {
        Coverage::None
    };
    LineEstimate {
        requested,
        covered,
        total: (requested == 0 || covered > 0).then_some(total),
        unit_price,
        coverage,
    }
}

pub fn estimate_cart<'a>(
    rows: impl IntoIterator<Item = (&'a ListItem, &'a [ActiveListing])>,
) -> CartEstimate {
    let mut cart = CartEstimate::default();
    for (item, listings) in rows {
        let line = estimate_line(item, listings);
        cart.lines += 1;
        cart.total = cart.total.saturating_add(line.total.unwrap_or(0));
        cart.requested_units = cart
            .requested_units
            .saturating_add(i64::from(line.requested));
        cart.covered_units = cart.covered_units.saturating_add(i64::from(line.covered));
        match line.coverage {
            Coverage::Full => {}
            Coverage::Partial => cart.short_lines += 1,
            Coverage::None => cart.unknown_lines += 1,
        }
    }
    cart
}

/// Development fixture: `quantity` units at `price` gil, per listing.
#[cfg(test)]
pub fn fixture_listing(id: i32, price: i32, quantity: i32, hq: bool) -> ActiveListing {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn item(quantity: i32, acquired: i32, hq: Option<bool>) -> ListItem {
        ListItem {
            id: 1,
            item_id: 1,
            list_id: 0,
            hq,
            quantity: Some(quantity),
            acquired: Some(acquired),
            target_price: None,
        }
    }

    #[test]
    fn fills_from_the_cheapest_listings_first() {
        let listings = [
            fixture_listing(1, 300, 5, false),
            fixture_listing(2, 100, 2, false),
            fixture_listing(3, 200, 1, false),
        ];
        let line = estimate_line(&item(4, 0, None), &listings);
        assert_eq!(line.requested, 4);
        assert_eq!(line.covered, 4);
        assert_eq!(line.total, Some(2 * 100 + 200 + 300));
        assert_eq!(line.unit_price, Some(100));
        assert_eq!(line.coverage, Coverage::Full);
    }

    #[test]
    fn owned_units_reduce_what_is_priced() {
        let listings = [fixture_listing(1, 50, 10, false)];
        let line = estimate_line(&item(6, 4, None), &listings);
        assert_eq!(line.requested, 2);
        assert_eq!(line.total, Some(100));
        let done = estimate_line(&item(3, 3, None), &listings);
        assert_eq!(done.requested, 0);
        assert_eq!(done.total, Some(0));
        assert_eq!(done.coverage, Coverage::Full);
    }

    #[test]
    fn quality_rows_only_take_matching_listings() {
        let listings = [
            fixture_listing(1, 10, 5, false),
            fixture_listing(2, 40, 5, true),
        ];
        let hq = estimate_line(&item(2, 0, Some(true)), &listings);
        assert_eq!(hq.total, Some(80));
        assert_eq!(hq.unit_price, Some(40));
        let nq = estimate_line(&item(2, 0, Some(false)), &listings);
        assert_eq!(nq.total, Some(20));
        let any = estimate_line(&item(7, 0, None), &listings);
        assert_eq!(any.total, Some(5 * 10 + 2 * 40));
        assert_eq!(any.coverage, Coverage::Full);
    }

    #[test]
    fn short_supply_is_partial_and_no_supply_is_unknown() {
        let short = estimate_line(&item(10, 0, None), &[fixture_listing(1, 7, 3, false)]);
        assert_eq!(short.covered, 3);
        assert_eq!(short.total, Some(21));
        assert_eq!(short.coverage, Coverage::Partial);
        let none = estimate_line(&item(2, 0, Some(true)), &[fixture_listing(1, 7, 3, false)]);
        assert_eq!(none.covered, 0);
        assert_eq!(none.total, None);
        assert_eq!(none.unit_price, None);
        assert_eq!(none.coverage, Coverage::None);
    }

    #[test]
    fn cart_aggregates_lines_and_reports_incompleteness() {
        let full = (item(2, 0, None), vec![fixture_listing(1, 100, 2, false)]);
        let partial = (item(5, 0, None), vec![fixture_listing(2, 10, 1, false)]);
        let unknown = (item(1, 0, Some(true)), vec![]);
        let done = (item(1, 1, None), vec![]);
        let rows = [full, partial, unknown, done];
        let cart = estimate_cart(rows.iter().map(|(item, l)| (item, l.as_slice())));
        assert_eq!(cart.lines, 4);
        assert_eq!(cart.total, 210);
        assert_eq!(cart.short_lines, 1);
        assert_eq!(cart.unknown_lines, 1);
        assert_eq!(cart.requested_units, 8);
        assert_eq!(cart.covered_units, 3);
        assert!(!cart.complete());
        let complete = estimate_cart([(&rows[0].0, rows[0].1.as_slice())]);
        assert!(complete.complete());
        assert_eq!(estimate_cart([]), CartEstimate::default());
        assert!(
            estimate_cart([]).complete(),
            "an empty cart has nothing uncovered"
        );
    }

    #[test]
    fn cart_total_saturates_instead_of_wrapping() {
        let listings = [fixture_listing(1, i32::MAX, i32::MAX, false)];
        let many = vec![(item(i32::MAX, 0, None), listings.to_vec()); 1000];
        let cart = estimate_cart(many.iter().map(|(item, l)| (item, l.as_slice())));
        assert_eq!(cart.total, i64::MAX);
        assert!(cart.complete());
    }

    #[test]
    fn huge_prices_saturate_instead_of_overflowing() {
        let listings = [
            fixture_listing(1, i32::MAX, i32::MAX, false),
            fixture_listing(2, i32::MAX, i32::MAX, false),
        ];
        let line = estimate_line(&item(i32::MAX, 0, None), &listings);
        assert_eq!(line.covered, i32::MAX);
        assert_eq!(line.total, Some(i64::from(i32::MAX) * i64::from(i32::MAX)));
    }

    #[test]
    fn negative_or_zero_stacks_are_ignored() {
        let listings = [
            fixture_listing(1, 5, 0, false),
            fixture_listing(2, 6, -3, false),
            fixture_listing(3, 9, 2, false),
        ];
        let line = estimate_line(&item(2, 0, None), &listings);
        assert_eq!(line.total, Some(18));
        assert_eq!(
            line.unit_price,
            Some(5),
            "cheapest observed price is still reported"
        );
    }
}
