//! Quantity-aware cost estimates for a list's cart.
//!
//! Build prices the units a player still needs, cheapest listed units
//! first, and reports how much of that need the board can actually cover.
//! The result is an *estimate* of market cost — partial stacks are allowed,
//! nothing is reserved — and it is deliberately distinct from the Shop
//! planner, which buys whole stacks and reports surplus.
//!
//! Contracts (see `docs/superpowers/specs/2026-09-10-lists-cart-estimates-design.md`):
//!
//! - The estimate prices the **remaining** quantity: `requested − acquired`,
//!   clamped to zero. A row with nothing remaining is [`LineStatus::Acquired`]
//!   and contributes nothing.
//! - NQ prices only NQ listings, HQ only HQ listings, `None` (Any) prices both
//!   together without preferring either.
//! - Units are taken from matching listings in ascending unit-price order
//!   until the remaining quantity is covered. When the board runs out the
//!   line is short ([`LineStatus::PartialSupply`] or [`LineStatus::NoSupply`]),
//!   its total covers only the priced units, and the cart is marked
//!   incomplete. A short line is never shown as free.
//! - Unit prices and quantities are `i32`, so every product and per-line sum
//!   fits an `i64`. Cart sums use checked arithmetic and saturate with
//!   [`CartEstimate::saturated`] set rather than wrapping.

use std::cmp::Reverse;

use chrono::{DateTime, Utc};
use ultros_api_types::{ActiveListing, list::ListItem};

/// One row's request, independent of how the row is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRequest {
    /// The row's identity in the document; carried through unchanged.
    pub row_id: i32,
    pub item_id: i32,
    /// `None` accepts either quality.
    pub hq: Option<bool>,
    /// Units the row asks for. Treated as at least one, like the editor.
    pub requested: i32,
    /// Units already owned. Negative values count as zero.
    pub acquired: i32,
}

impl LineRequest {
    /// Units still to buy: `requested − acquired`, never below zero, with
    /// `requested` floored at one to match the row editor's validation.
    pub fn remaining(&self) -> i32 {
        let requested = self.requested.max(1);
        requested.saturating_sub(self.acquired.clamp(0, requested))
    }
}

impl From<&ListItem> for LineRequest {
    fn from(item: &ListItem) -> Self {
        Self {
            row_id: item.id,
            item_id: item.item_id,
            hq: item.hq,
            requested: item.quantity.unwrap_or(1),
            acquired: item.acquired.unwrap_or(0),
        }
    }
}

/// How much of a line's remaining need the estimate covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStatus {
    /// Nothing remaining; the line prices nothing.
    Acquired,
    /// Every remaining unit has a listed price.
    Priced,
    /// Some remaining units have a price; the rest have no matching supply.
    PartialSupply,
    /// No matching listing at all.
    NoSupply,
}

impl LineStatus {
    /// True when the line still needs units the board cannot price.
    pub fn is_short(self) -> bool {
        matches!(self, Self::PartialSupply | Self::NoSupply)
    }
}

/// One row's estimate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineEstimate {
    pub row_id: i32,
    pub item_id: i32,
    pub hq: Option<bool>,
    pub requested: i32,
    pub remaining: i32,
    /// Units the total covers.
    pub priced_units: i32,
    /// Remaining units with no matching listing.
    pub unpriced_units: i32,
    /// Gil for the priced units only.
    pub total: i64,
    /// Cheapest matching unit price — what the existing price column shows.
    pub unit_price: Option<i32>,
    pub status: LineStatus,
}

/// Coverage of the whole cart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Coverage {
    /// No line needs units: the list is empty or everything is acquired.
    #[default]
    Empty,
    /// Every line that needs units is fully priced.
    Complete,
    /// At least one line is priced and at least one is short.
    Partial,
    /// Every line that needs units is short.
    None,
}

/// The cart's estimate: line results plus the aggregate the total shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CartEstimate {
    pub lines: Vec<LineEstimate>,
    /// Gil for every priced unit across the cart. Saturates rather than
    /// wrapping; see [`Self::saturated`].
    pub total: i64,
    /// A cart-level sum exceeded `i64`; `total` is a floor, not the cost.
    pub saturated: bool,
    pub coverage: Coverage,
    /// Lines whose remaining units are fully priced.
    pub lines_priced: usize,
    /// Lines with remaining units the board cannot fully price.
    pub lines_short: usize,
    /// Lines with nothing remaining.
    pub lines_acquired: usize,
    /// Remaining units across the cart with no listed price.
    pub unpriced_units: i64,
}

impl CartEstimate {
    /// Lines that still need units, priced or not.
    pub fn lines_needing_units(&self) -> usize {
        self.lines_priced + self.lines_short
    }

    /// True unless every needed unit is priced and the total is exact.
    pub fn is_incomplete(&self) -> bool {
        self.saturated || matches!(self.coverage, Coverage::Partial | Coverage::None)
    }
}

/// Estimate one row from the listings the page holds for its item.
///
/// Listings for another item, the wrong quality, an empty stack, or a
/// non-positive price are ignored. Units are taken cheapest-first; equal
/// prices prefer the larger stack and then the lower listing id so the
/// result is stable across renders.
pub fn estimate_line(request: LineRequest, listings: &[ActiveListing]) -> LineEstimate {
    let remaining = request.remaining();
    let mut matching: Vec<&ActiveListing> = listings
        .iter()
        .filter(|listing| listing.item_id == request.item_id)
        .filter(|listing| request.hq.is_none_or(|hq| listing.hq == hq))
        .filter(|listing| listing.quantity > 0 && listing.price_per_unit > 0)
        .collect();
    matching.sort_by_key(|listing| {
        (
            listing.price_per_unit,
            Reverse(listing.quantity),
            listing.id,
        )
    });
    let unit_price = matching.first().map(|listing| listing.price_per_unit);

    let mut left = i64::from(remaining);
    let mut total: i64 = 0;
    for listing in &matching {
        if left == 0 {
            break;
        }
        let take = i64::from(listing.quantity).min(left);
        // `i32 × i32` fits `i64`, and the running sum is bounded by
        // `remaining × max unit price`, which also fits.
        total += i64::from(listing.price_per_unit) * take;
        left -= take;
    }
    let unpriced_units = left as i32;
    let priced_units = remaining - unpriced_units;
    let status = if remaining == 0 {
        LineStatus::Acquired
    } else if priced_units == 0 {
        LineStatus::NoSupply
    } else if unpriced_units > 0 {
        LineStatus::PartialSupply
    } else {
        LineStatus::Priced
    };
    LineEstimate {
        row_id: request.row_id,
        item_id: request.item_id,
        hq: request.hq,
        requested: request.requested,
        remaining,
        priced_units,
        unpriced_units,
        total,
        unit_price,
        status,
    }
}

/// Estimate every row and aggregate the cart.
pub fn estimate_cart<'a, I>(rows: I) -> CartEstimate
where
    I: IntoIterator<Item = (LineRequest, &'a [ActiveListing])>,
{
    let mut cart = CartEstimate::default();
    for (request, listings) in rows {
        let line = estimate_line(request, listings);
        match line.status {
            LineStatus::Acquired => cart.lines_acquired += 1,
            LineStatus::Priced => cart.lines_priced += 1,
            LineStatus::PartialSupply | LineStatus::NoSupply => cart.lines_short += 1,
        }
        cart.total = match cart.total.checked_add(line.total) {
            Some(total) => total,
            None => {
                cart.saturated = true;
                i64::MAX
            }
        };
        cart.unpriced_units = cart
            .unpriced_units
            .saturating_add(i64::from(line.unpriced_units));
        cart.lines.push(line);
    }
    cart.coverage = match (cart.lines_priced, cart.lines_short) {
        (0, 0) => Coverage::Empty,
        (_, 0) => Coverage::Complete,
        (0, _) => Coverage::None,
        _ => Coverage::Partial,
    };
    cart
}

/// Estimate the `(row, listings)` pairs the list pages already hold.
pub fn estimate_list_items(rows: &[(ListItem, Vec<ActiveListing>)]) -> CartEstimate {
    estimate_cart(
        rows.iter()
            .map(|(item, listings)| (LineRequest::from(item), listings.as_slice())),
    )
}

/// Why an estimate has no listings behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingReason {
    /// Nobody has asked for prices yet (a device list before its Shop
    /// lookup).
    NotRequested,
    /// The lookup failed and nothing earlier is cached.
    Failed,
}

/// The listings behind an estimate: whether any exist and when they arrived.
///
/// `fetched_at` is the *client-clock* instant a listing response was
/// received — the REST read for an account list (including the refetches a
/// market broadcast triggers), the Shop lookup for a device list. It is not
/// an ingest time and must never be derived from a listing's own
/// `timestamp`, which is a seller review time. A failed refresh keeps the
/// previous observation and marks it, so the page can say "prices from
/// <time>; refresh failed" instead of pretending the failure produced fresh
/// prices, and an offline edit keeps whatever was last seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceFeed {
    /// A lookup is in flight (or about to be) and nothing earlier exists.
    Loading,
    /// No listings, and why.
    Missing(MissingReason),
    /// Listings from a completed lookup.
    Observed {
        fetched_at: DateTime<Utc>,
        /// A later lookup failed; these listings are older than intended.
        refresh_failed: bool,
    },
}

impl PriceFeed {
    pub fn observed(fetched_at: DateTime<Utc>) -> Self {
        Self::Observed {
            fetched_at,
            refresh_failed: false,
        }
    }

    /// A lookup is starting. Prices already observed stay in place — the
    /// estimate keeps working through a refresh — while a feed with nothing
    /// to show reports that it is loading.
    pub fn begin_fetch(self) -> Self {
        match self {
            Self::Observed { .. } => self,
            Self::Loading | Self::Missing(_) => Self::Loading,
        }
    }

    /// The lookup finished. `Some(instant)` replaces whatever was there;
    /// `None` (a failure) keeps an earlier observation, marked, and
    /// otherwise records that there is nothing to show.
    pub fn after_fetch(self, fetched_at: Option<DateTime<Utc>>) -> Self {
        match (self, fetched_at) {
            (_, Some(fetched_at)) => Self::observed(fetched_at),
            (Self::Observed { fetched_at, .. }, None) => Self::Observed {
                fetched_at,
                refresh_failed: true,
            },
            (Self::Loading | Self::Missing(_), None) => Self::Missing(MissingReason::Failed),
        }
    }

    pub fn fetched_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Observed { fetched_at, .. } => Some(*fetched_at),
            Self::Loading | Self::Missing(_) => None,
        }
    }

    /// True when there are listings to estimate from, fresh or marked.
    pub fn has_prices(&self) -> bool {
        matches!(self, Self::Observed { .. })
    }
}

/// Discards a lookup that lands after a newer one began — the player changed
/// scope or list while the request was in flight — so a late response can
/// never overwrite the prices for the scope they are now looking at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LookupTicket(u64);

impl LookupTicket {
    /// Start a lookup, invalidating every earlier ticket.
    pub fn begin(&mut self) -> Self {
        self.0 = self.0.wrapping_add(1);
        *self
    }

    /// Whether `ticket` is still the newest lookup.
    pub fn accepts(&self, ticket: Self) -> bool {
        *self == ticket
    }
}

/// Ready-made carts for the presentation track to build against before its
/// integration lands. Each is produced by the real estimator from synthetic
/// listings, so a fixture can never disagree with the arithmetic.
pub mod fixtures {
    use super::*;
    use chrono::NaiveDateTime;

    /// A listing with only the fields the estimator reads set meaningfully.
    pub fn listing(id: i32, item_id: i32, hq: bool, price: i32, quantity: i32) -> ActiveListing {
        ActiveListing {
            id,
            world_id: 1,
            item_id,
            retainer_id: 0,
            price_per_unit: price,
            quantity,
            hq,
            timestamp: NaiveDateTime::default(),
        }
    }

    pub fn request(
        row_id: i32,
        item_id: i32,
        hq: Option<bool>,
        requested: i32,
        acquired: i32,
    ) -> LineRequest {
        LineRequest {
            row_id,
            item_id,
            hq,
            requested,
            acquired,
        }
    }

    /// Three rows, every unit priced: 3 × 100 + 2 × 250 + (1 × 900 + 1 × 1_000).
    pub fn complete() -> CartEstimate {
        let a = [listing(1, 10, false, 100, 10)];
        let b = [listing(2, 20, true, 250, 5)];
        let c = [
            listing(3, 30, false, 900, 1),
            listing(4, 30, true, 1_000, 4),
        ];
        estimate_cart([
            (request(1, 10, Some(false), 3, 0), &a[..]),
            (request(2, 20, Some(true), 2, 0), &b[..]),
            (request(3, 30, None, 2, 0), &c[..]),
        ])
    }

    /// One priced row, one short row, one row with no supply, one acquired.
    pub fn partial() -> CartEstimate {
        let a = [listing(1, 10, false, 100, 10)];
        let b = [listing(2, 20, true, 250, 1)];
        let c: [ActiveListing; 0] = [];
        let d = [listing(3, 40, false, 5, 5)];
        estimate_cart([
            (request(1, 10, Some(false), 3, 0), &a[..]),
            (request(2, 20, Some(true), 4, 0), &b[..]),
            (request(3, 30, None, 2, 0), &c[..]),
            (request(4, 40, None, 5, 5), &d[..]),
        ])
    }

    /// Rows that need units, none of which has a listing.
    pub fn unpriced() -> CartEstimate {
        let none: [ActiveListing; 0] = [];
        estimate_cart([
            (request(1, 10, None, 1, 0), &none[..]),
            (request(2, 20, Some(true), 3, 1), &none[..]),
        ])
    }

    /// Everything acquired.
    pub fn empty() -> CartEstimate {
        let a = [listing(1, 10, false, 100, 10)];
        estimate_cart([(request(1, 10, None, 2, 2), &a[..])])
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{listing, request};
    use super::*;

    fn line(request: LineRequest, listings: &[ActiveListing]) -> LineEstimate {
        estimate_line(request, listings)
    }

    #[test]
    fn remaining_is_requested_minus_acquired_clamped() {
        assert_eq!(request(1, 1, None, 5, 2).remaining(), 3);
        assert_eq!(request(1, 1, None, 5, 9).remaining(), 0);
        assert_eq!(request(1, 1, None, 5, -3).remaining(), 5);
        // The editor never stores less than one; a zero request prices one.
        assert_eq!(request(1, 1, None, 0, 0).remaining(), 1);
    }

    #[test]
    fn list_item_defaults_match_the_page() {
        let item = ListItem {
            id: 7,
            item_id: 42,
            list_id: 1,
            hq: Some(true),
            quantity: None,
            acquired: None,
            target_price: None,
        };
        let request = LineRequest::from(&item);
        assert_eq!(request.requested, 1);
        assert_eq!(request.acquired, 0);
        assert_eq!(request.remaining(), 1);
        assert_eq!(request.hq, Some(true));
        assert_eq!(request.row_id, 7);
    }

    #[test]
    fn cheapest_units_are_taken_across_stacks() {
        // Cheapest stack is too small; the rest comes from the next price.
        let listings = [
            listing(1, 10, false, 1_000, 99),
            listing(2, 10, false, 100, 1),
        ];
        let estimate = line(request(1, 10, None, 100, 0), &listings);
        assert_eq!(estimate.total, 100 + 99 * 1_000);
        assert_eq!(estimate.unit_price, Some(100));
        assert_eq!(estimate.status, LineStatus::Priced);
        assert_eq!(estimate.priced_units, 100);
        assert_eq!(estimate.unpriced_units, 0);
    }

    #[test]
    fn partial_stacks_are_priced_not_whole_stacks() {
        let listings = [listing(1, 10, false, 100, 99)];
        let estimate = line(request(1, 10, None, 3, 0), &listings);
        assert_eq!(
            estimate.total, 300,
            "Build prices units, Shop prices stacks"
        );
    }

    #[test]
    fn quality_filters_listings() {
        let listings = [
            listing(1, 10, false, 100, 10),
            listing(2, 10, true, 500, 10),
        ];
        let nq = line(request(1, 10, Some(false), 2, 0), &listings);
        let hq = line(request(1, 10, Some(true), 2, 0), &listings);
        let any = line(request(1, 10, None, 12, 0), &listings);
        assert_eq!(nq.total, 200);
        assert_eq!(hq.total, 1_000);
        // Any takes the cheapest units of either quality, not one quality.
        assert_eq!(any.total, 10 * 100 + 2 * 500);
        assert_eq!(any.status, LineStatus::Priced);
    }

    #[test]
    fn hq_request_with_only_nq_supply_has_no_supply() {
        let listings = [listing(1, 10, false, 100, 10)];
        let estimate = line(request(1, 10, Some(true), 2, 0), &listings);
        assert_eq!(estimate.status, LineStatus::NoSupply);
        assert_eq!(estimate.total, 0);
        assert_eq!(estimate.unit_price, None);
        assert_eq!(estimate.unpriced_units, 2);
    }

    #[test]
    fn partially_acquired_rows_price_only_the_rest() {
        let listings = [listing(1, 10, false, 100, 10)];
        let estimate = line(request(1, 10, None, 5, 3), &listings);
        assert_eq!(estimate.requested, 5);
        assert_eq!(estimate.remaining, 2);
        assert_eq!(estimate.total, 200);
        assert_eq!(estimate.status, LineStatus::Priced);
    }

    #[test]
    fn acquired_rows_price_nothing_even_with_supply() {
        let listings = [listing(1, 10, false, 100, 10)];
        let estimate = line(request(1, 10, None, 5, 5), &listings);
        assert_eq!(estimate.status, LineStatus::Acquired);
        assert_eq!(estimate.total, 0);
        assert_eq!(estimate.remaining, 0);
        // The cheapest price is still reported for the price column.
        assert_eq!(estimate.unit_price, Some(100));
    }

    #[test]
    fn short_supply_prices_what_exists_and_counts_the_rest() {
        let listings = [listing(1, 10, false, 100, 3)];
        let estimate = line(request(1, 10, None, 10, 0), &listings);
        assert_eq!(estimate.status, LineStatus::PartialSupply);
        assert_eq!(estimate.priced_units, 3);
        assert_eq!(estimate.unpriced_units, 7);
        assert_eq!(estimate.total, 300);
    }

    #[test]
    fn listings_for_other_items_and_bogus_listings_are_ignored() {
        let listings = [
            listing(1, 11, false, 1, 100),
            listing(2, 10, false, 0, 100),
            listing(3, 10, false, 100, 0),
            listing(4, 10, false, 100, 2),
        ];
        let estimate = line(request(1, 10, None, 2, 0), &listings);
        assert_eq!(estimate.total, 200);
        assert_eq!(estimate.unit_price, Some(100));
    }

    #[test]
    fn equal_prices_prefer_larger_then_lower_id_deterministically() {
        let listings = [
            listing(9, 10, false, 100, 1),
            listing(3, 10, false, 100, 5),
            listing(1, 10, false, 100, 5),
        ];
        let forward = line(request(1, 10, None, 6, 0), &listings);
        let mut reversed = listings.to_vec();
        reversed.reverse();
        let backward = line(request(1, 10, None, 6, 0), &reversed);
        assert_eq!(forward, backward);
        assert_eq!(forward.total, 600);
    }

    #[test]
    fn cart_mixes_priced_short_unpriced_and_acquired_lines() {
        let cart = fixtures::partial();
        assert_eq!(cart.coverage, Coverage::Partial);
        assert_eq!(cart.lines_priced, 1);
        assert_eq!(cart.lines_short, 2);
        assert_eq!(cart.lines_acquired, 1);
        assert_eq!(cart.lines_needing_units(), 3);
        // 3 × 100 for the priced row plus 1 × 250 for the short row.
        assert_eq!(cart.total, 550);
        assert_eq!(cart.unpriced_units, 3 + 2);
        assert!(cart.is_incomplete());
        assert!(!cart.saturated);
    }

    #[test]
    fn complete_cart_is_complete() {
        let cart = fixtures::complete();
        assert_eq!(cart.coverage, Coverage::Complete);
        assert_eq!(cart.total, 300 + 500 + 900 + 1_000);
        assert!(!cart.is_incomplete());
        assert_eq!(cart.unpriced_units, 0);
    }

    #[test]
    fn cart_with_no_supply_is_none_not_zero_cost() {
        let cart = fixtures::unpriced();
        assert_eq!(cart.coverage, Coverage::None);
        assert_eq!(cart.total, 0);
        assert_eq!(cart.unpriced_units, 1 + 2);
        assert!(cart.is_incomplete());
    }

    #[test]
    fn fully_acquired_or_empty_cart_is_empty() {
        assert_eq!(fixtures::empty().coverage, Coverage::Empty);
        assert!(!fixtures::empty().is_incomplete());
        let none: Vec<(LineRequest, &[ActiveListing])> = Vec::new();
        assert_eq!(estimate_cart(none), CartEstimate::default());
    }

    #[test]
    fn scope_change_reprices_the_same_rows() {
        // The estimator has no scope of its own: handing it a different
        // listing set for the same rows is exactly a scope change.
        let rows = [request(1, 10, None, 4, 0), request(2, 20, Some(true), 1, 0)];
        let home = [
            vec![listing(1, 10, false, 100, 10)],
            vec![listing(2, 20, true, 900, 1)],
        ];
        let region = [
            vec![listing(3, 10, false, 60, 2), listing(1, 10, false, 100, 10)],
            Vec::new(),
        ];
        let at_home = estimate_cart(rows.iter().copied().zip(home.iter().map(Vec::as_slice)));
        let in_region = estimate_cart(rows.iter().copied().zip(region.iter().map(Vec::as_slice)));
        assert_eq!(at_home.total, 400 + 900);
        assert_eq!(at_home.coverage, Coverage::Complete);
        assert_eq!(in_region.total, 2 * 60 + 2 * 100);
        assert_eq!(in_region.coverage, Coverage::Partial);
    }

    #[test]
    fn a_maximal_single_line_fits_without_saturating() {
        let listings = [listing(1, 10, false, i32::MAX, i32::MAX)];
        let estimate = line(request(1, 10, None, i32::MAX, 0), &listings);
        assert_eq!(estimate.total, i64::from(i32::MAX) * i64::from(i32::MAX));
        assert_eq!(estimate.status, LineStatus::Priced);
        let cart = estimate_cart([(request(1, 10, None, i32::MAX, 0), &listings[..])]);
        assert!(!cart.saturated);
        assert_eq!(cart.total, estimate.total);
    }

    #[test]
    fn a_maximal_line_split_across_stacks_still_fits() {
        // Many small stacks at the maximum price: the per-line sum is
        // bounded by remaining × price and must not overflow mid-loop.
        let listings: Vec<ActiveListing> = (0..8)
            .map(|id| listing(id, 10, false, i32::MAX, i32::MAX / 4))
            .collect();
        let estimate = line(request(1, 10, None, i32::MAX, 0), &listings);
        assert_eq!(estimate.total, i64::from(i32::MAX) * i64::from(i32::MAX));
        assert_eq!(estimate.status, LineStatus::Priced);
    }

    #[test]
    fn cart_sum_saturates_instead_of_wrapping() {
        let listings = [listing(1, 10, false, i32::MAX, i32::MAX)];
        let maximal = request(1, 10, None, i32::MAX, 0);
        let two = estimate_cart([(maximal, &listings[..]), (maximal, &listings[..])]);
        assert!(!two.saturated, "two maximal lines still fit an i64");
        let three = estimate_cart([
            (maximal, &listings[..]),
            (maximal, &listings[..]),
            (maximal, &listings[..]),
        ]);
        assert!(three.saturated);
        assert_eq!(three.total, i64::MAX);
        assert!(three.is_incomplete());
        // Coverage is still reported from the lines themselves.
        assert_eq!(three.coverage, Coverage::Complete);
    }

    #[test]
    fn list_item_rows_estimate_like_requests() {
        let item = ListItem {
            id: 1,
            item_id: 10,
            list_id: 5,
            hq: None,
            quantity: Some(3),
            acquired: Some(1),
            target_price: None,
        };
        let rows = vec![(item, vec![listing(1, 10, false, 100, 10)])];
        let cart = estimate_list_items(&rows);
        assert_eq!(cart.total, 200);
        assert_eq!(cart.lines[0].row_id, 1);
        assert_eq!(cart.lines[0].remaining, 2);
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
    }

    #[test]
    fn a_first_fetch_observes_prices() {
        let feed = PriceFeed::Loading.after_fetch(Some(at(100)));
        assert_eq!(feed, PriceFeed::observed(at(100)));
        assert_eq!(feed.fetched_at(), Some(at(100)));
        assert!(feed.has_prices());
    }

    #[test]
    fn a_failed_first_fetch_has_nothing_to_show() {
        let feed = PriceFeed::Loading.after_fetch(None);
        assert_eq!(feed, PriceFeed::Missing(MissingReason::Failed));
        assert!(!feed.has_prices());
        assert_eq!(feed.fetched_at(), None);
        let never = PriceFeed::Missing(MissingReason::NotRequested).after_fetch(None);
        assert_eq!(never, PriceFeed::Missing(MissingReason::Failed));
    }

    #[test]
    fn a_failed_refresh_keeps_the_earlier_prices_and_marks_them() {
        // Offline, a 5xx, a dropped socket: the cart keeps estimating from
        // what it last saw, and says those prices are older than intended.
        let feed = PriceFeed::observed(at(100)).after_fetch(None);
        assert_eq!(
            feed,
            PriceFeed::Observed {
                fetched_at: at(100),
                refresh_failed: true,
            }
        );
        assert!(feed.has_prices());
        assert_eq!(feed.fetched_at(), Some(at(100)));
        // The next successful fetch clears the mark and moves the clock.
        assert_eq!(
            feed.after_fetch(Some(at(200))),
            PriceFeed::observed(at(200))
        );
    }

    #[test]
    fn beginning_a_fetch_only_changes_a_feed_with_nothing_to_show() {
        assert_eq!(
            PriceFeed::Missing(MissingReason::NotRequested).begin_fetch(),
            PriceFeed::Loading
        );
        assert_eq!(
            PriceFeed::Missing(MissingReason::Failed).begin_fetch(),
            PriceFeed::Loading
        );
        assert_eq!(PriceFeed::Loading.begin_fetch(), PriceFeed::Loading);
        let stale = PriceFeed::Observed {
            fetched_at: at(100),
            refresh_failed: true,
        };
        assert_eq!(
            stale.begin_fetch(),
            stale,
            "a refresh never blanks the cart"
        );
    }

    #[test]
    fn a_lookup_that_lands_after_a_newer_one_began_is_rejected() {
        let mut ticket = LookupTicket::default();
        let first = ticket.begin();
        assert!(ticket.accepts(first));
        // The player changed scope: a second lookup starts before the first
        // returns.
        let second = ticket.begin();
        assert!(
            !ticket.accepts(first),
            "the old scope's prices must not land"
        );
        assert!(ticket.accepts(second));
        // Applying the guarded transition twice out of order leaves the
        // feed on the newer result only.
        let mut feed = PriceFeed::Loading;
        if ticket.accepts(second) {
            feed = feed.after_fetch(Some(at(200)));
        }
        if ticket.accepts(first) {
            feed = feed.after_fetch(Some(at(100)));
        }
        assert_eq!(feed, PriceFeed::observed(at(200)));
    }
}
