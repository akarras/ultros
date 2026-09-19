//! Vendor Sell: market listings priced below the NPC vendor sell-back price.
//!
//! Spec: docs/superpowers/specs/2026-09-18-vendor-sell-analyzer-design.md

use crate::components::sort_header::{SortColumn, SortDir};
use crate::global_state::xiv_data::tracked_data;
use std::{cmp::Ordering, collections::HashMap, sync::Arc};
use ultros_api_types::cheapest_listings::CheapestListings;
use ultros_calc::formula::vendor_sell_line;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VendorSellRow {
    pub item_id: i32,
    pub hq: bool,
    pub world_id: i32,
    pub listing: i32,
    pub tax: i32,
    pub cost: i32,
    pub vendor_price: i32,
    pub profit: i32,
    pub margin: i32,
}

/// `item_id -> price_low` for every item a player can both buy on the board
/// and sell to an NPC.
fn vendor_prices_from_data() -> HashMap<i32, i32> {
    tracked_data()
        .items
        .iter()
        .filter(|(_, item)| item.item_search_category != 0 && item.price_low > 0)
        .map(|(id, item)| (id.0, item.price_low as i32))
        .collect()
}

/// Join listings against vendor prices, drop anything that does not profit,
/// and return a deterministic (item id, then NQ before HQ) order.
fn build_rows(
    listings: &CheapestListings,
    vendor_prices: &HashMap<i32, i32>,
) -> Vec<Arc<VendorSellRow>> {
    let mut rows: Vec<Arc<VendorSellRow>> = listings
        .cheapest_listings
        .iter()
        .filter_map(|listing| {
            let vendor_price = *vendor_prices.get(&listing.item_id)?;
            let line = vendor_sell_line(listing.cheapest_price, vendor_price);
            (line.profit > 0).then(|| {
                Arc::new(VendorSellRow {
                    item_id: listing.item_id,
                    hq: listing.hq,
                    world_id: listing.world_id,
                    listing: line.listing,
                    tax: line.tax,
                    cost: line.cost,
                    vendor_price: line.vendor_price,
                    profit: line.profit,
                    margin: line.margin,
                })
            })
        })
        .collect();
    rows.sort_by_key(|row| (row.item_id, row.hq));
    rows
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Profit,
    Margin,
    Listing,
    Tax,
    VendorPrice,
    World,
}

impl std::str::FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "profit" => Ok(SortMode::Profit),
            "margin" => Ok(SortMode::Margin),
            "listing" => Ok(SortMode::Listing),
            "tax" => Ok(SortMode::Tax),
            "vendor-price" => Ok(SortMode::VendorPrice),
            "world" => Ok(SortMode::World),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SortMode::Profit => "profit",
            SortMode::Margin => "margin",
            SortMode::Listing => "listing",
            SortMode::Tax => "tax",
            SortMode::VendorPrice => "vendor-price",
            SortMode::World => "world",
        })
    }
}

impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Profit
    }

    /// Money columns read best-first descending; the cost-like columns
    /// (listing, tax) and world read ascending.
    fn default_dir(self) -> SortDir {
        match self {
            SortMode::Profit | SortMode::Margin | SortMode::VendorPrice => SortDir::Desc,
            SortMode::Listing | SortMode::Tax | SortMode::World => SortDir::Asc,
        }
    }
}

fn compare_rows(mode: SortMode, a: &VendorSellRow, b: &VendorSellRow) -> Ordering {
    match mode {
        SortMode::Profit => a.profit.cmp(&b.profit),
        SortMode::Margin => a.margin.cmp(&b.margin),
        SortMode::Listing => a.listing.cmp(&b.listing),
        SortMode::Tax => a.tax.cmp(&b.tax),
        SortMode::VendorPrice => a.vendor_price.cmp(&b.vendor_price),
        SortMode::World => a.world_id.cmp(&b.world_id),
    }
    .then_with(|| a.item_id.cmp(&b.item_id))
    .then_with(|| a.hq.cmp(&b.hq))
}

fn sort_rows(rows: &mut [Arc<VendorSellRow>], mode: SortMode, dir: SortDir) {
    rows.sort_by(|a, b| {
        let order = compare_rows(mode, a, b);
        if dir == SortDir::Asc {
            order
        } else {
            order.reverse()
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use ultros_api_types::cheapest_listings::CheapestListingItem;

    fn listing(item_id: i32, hq: bool, cheapest_price: i32, world_id: i32) -> CheapestListingItem {
        CheapestListingItem {
            item_id,
            hq,
            cheapest_price,
            world_id,
        }
    }

    fn listings(items: Vec<CheapestListingItem>) -> CheapestListings {
        CheapestListings {
            cheapest_listings: items,
        }
    }

    #[test]
    fn items_without_a_vendor_price_never_become_rows() {
        // item 2 is absent from the catalog: unmarketable or price_low == 0.
        let rows = build_rows(
            &listings(vec![listing(1, false, 10, 7), listing(2, false, 10, 7)]),
            &[(1, 100)].into_iter().collect(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item_id, 1);
    }

    #[test]
    fn unprofitable_rows_are_dropped() {
        // 100 + 5 tax = 105 cost. Vendor 105 → profit 0 → dropped.
        let rows = build_rows(
            &listings(vec![listing(1, false, 100, 7), listing(2, false, 100, 7)]),
            &[(1, 105), (2, 106)].into_iter().collect(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item_id, 2);
        assert_eq!(rows[0].profit, 1);
    }

    #[test]
    fn hq_listing_is_valued_at_nq_price_and_keeps_its_flag_and_world() {
        let rows = build_rows(
            &listings(vec![listing(1, true, 101, 42), listing(1, false, 200, 43)]),
            &[(1, 120)].into_iter().collect(),
        );
        // NQ row at 200 costs 210 > 120 → dropped; HQ row survives.
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(row.hq);
        assert_eq!(row.world_id, 42);
        assert_eq!(row.listing, 101);
        assert_eq!(row.tax, 6);
        assert_eq!(row.cost, 107);
        assert_eq!(row.vendor_price, 120);
        assert_eq!(row.profit, 13);
        assert_eq!(row.margin, 12);
    }

    #[test]
    fn build_rows_order_is_item_then_nq_before_hq() {
        let rows = build_rows(
            &listings(vec![
                listing(5, true, 1, 1),
                listing(3, false, 1, 1),
                listing(5, false, 1, 1),
            ]),
            &[(3, 100), (5, 100)].into_iter().collect(),
        );
        let keys: Vec<_> = rows.iter().map(|r| (r.item_id, r.hq)).collect();
        assert_eq!(keys, vec![(3, false), (5, false), (5, true)]);
    }

    #[test]
    fn default_sort_is_profit_descending_and_asc_reverses() {
        let mut rows = build_rows(
            &listings(vec![
                listing(1, false, 10, 1),
                listing(2, false, 10, 1),
                listing(3, false, 10, 1),
            ]),
            &[(1, 50), (2, 500), (3, 20)].into_iter().collect(),
        );
        let mode = SortMode::fallback();
        assert_eq!(mode, SortMode::Profit);
        assert_eq!(mode.default_dir(), SortDir::Desc);
        sort_rows(&mut rows, mode, mode.default_dir());
        let ids: Vec<_> = rows.iter().map(|r| r.item_id).collect();
        assert_eq!(ids, vec![2, 1, 3]);
        sort_rows(&mut rows, mode, SortDir::Asc);
        let ids: Vec<_> = rows.iter().map(|r| r.item_id).collect();
        assert_eq!(ids, vec![3, 1, 2]);
    }

    #[test]
    fn sort_tokens_round_trip() {
        for token in [
            "profit",
            "margin",
            "listing",
            "tax",
            "vendor-price",
            "world",
        ] {
            assert_eq!(SortMode::from_str(token).unwrap().to_string(), token);
        }
        assert!(SortMode::from_str("roi").is_err());
    }
}
