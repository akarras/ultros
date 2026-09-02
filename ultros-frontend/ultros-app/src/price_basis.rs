//! Selectable price bases for the recipe analyzer (#1202, #1233).
//!
//! The enums moved to [`crate::analyzer_kit::formula`] (one `PriceSignal`
//! for both sides); this module re-exports them so existing call sites
//! keep their names, and keeps the two map-cloning helpers that the
//! zero-copy `SignalView` reproduces.

pub use crate::analyzer_kit::formula::{BuyScope, CostBasis, RevenueMetric, SaleStat};
use ultros_api_types::cheapest_listings::{
    CheapestListingData, CheapestListingMapKey, CheapestListingsMap,
};
use ultros_api_types::sale_stats::BulkSaleStats;

/// Base map with every entry the override map carries replacing the base's.
/// Used for revenue: the sell world's own listing wins even when it is
/// higher than the buy-scope minimum (it is the price you would list at);
/// items with no sell-world listing keep the base price so rows aren't
/// dropped as unpriceable — same fallback the old `world-min` metric had.
pub fn override_listings(
    base: &CheapestListingsMap,
    over: &CheapestListingsMap,
) -> CheapestListingsMap {
    let mut map = base.map.clone();
    for (key, data) in &over.map {
        map.insert(*key, *data);
    }
    CheapestListingsMap { map }
}

/// Re-price a [`CheapestListingsMap`] from sale statistics.
///
/// Starts from the current-listings map and overrides each `(item, hq)`
/// entry that has sale history with the chosen statistic. Items with no
/// sales in the window keep their current-listing price — falling through
/// to 0 would fake "free" ingredients and corrupt the profit ranking.
/// Items with sales but no current listing gain an entry (they are still
/// buyable in practice; a missing entry would price them at 0 too).
///
/// `world_id` is preserved from the listing entry where one exists (it
/// feeds "cheapest world" UI); stat-only entries get `0`, which no UI
/// resolves to a world link.
pub fn overlay_sale_stats(
    listings: &CheapestListingsMap,
    stats: &BulkSaleStats,
    stat: SaleStat,
) -> CheapestListingsMap {
    let mut map = listings.map.clone();
    for row in &stats.stats {
        let price = match stat {
            SaleStat::Min => row.min_price,
            SaleStat::Median => row.median_price,
            SaleStat::Avg => row.avg_price,
        };
        if price <= 0 {
            continue;
        }
        let key = CheapestListingMapKey {
            item_id: row.item_id,
            hq: row.hq,
        };
        let world_id = map.get(&key).map(|d| d.world_id).unwrap_or(0);
        map.insert(key, CheapestListingData { price, world_id });
    }
    CheapestListingsMap { map }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};
    use ultros_api_types::sale_stats::ItemSaleStats;

    fn listings(items: &[(i32, bool, i32, i32)]) -> CheapestListingsMap {
        CheapestListingsMap::from(CheapestListings {
            cheapest_listings: items
                .iter()
                .map(
                    |&(item_id, hq, cheapest_price, world_id)| CheapestListingItem {
                        item_id,
                        hq,
                        cheapest_price,
                        world_id,
                    },
                )
                .collect(),
        })
    }

    fn stats(rows: &[(i32, bool, i32, i32, i32)]) -> BulkSaleStats {
        BulkSaleStats {
            stats: rows
                .iter()
                .map(
                    |&(item_id, hq, min_price, median_price, avg_price)| ItemSaleStats {
                        item_id,
                        hq,
                        min_price,
                        median_price,
                        avg_price,
                        num_sold: 10,
                        ..Default::default()
                    },
                )
                .collect(),
        }
    }

    #[test]
    fn url_values_round_trip() {
        for basis in [
            CostBasis::ListingMin,
            CostBasis::SaleMedian,
            CostBasis::SaleMin,
            CostBasis::SaleAvg,
        ] {
            assert_eq!(basis.to_string().parse(), Ok(basis));
        }
        for metric in [
            RevenueMetric::ListingMin,
            RevenueMetric::SaleMedian,
            RevenueMetric::SaleMin,
            RevenueMetric::SaleAvg,
        ] {
            assert_eq!(metric.to_string().parse(), Ok(metric));
        }
        for scope in [BuyScope::World, BuyScope::Datacenter, BuyScope::Region] {
            assert_eq!(scope.to_string().parse(), Ok(scope));
        }
    }

    #[test]
    fn defaults() {
        assert_eq!(CostBasis::default(), CostBasis::ListingMin);
        // Revenue is always evaluated on the sell world; the default is its
        // cheapest current listing — the price you'd actually list at.
        assert_eq!(RevenueMetric::default(), RevenueMetric::ListingMin);
        // Buying defaults to the datacenter: a realistic "purchase zone"
        // that doesn't assume cross-region travel.
        assert_eq!(BuyScope::default(), BuyScope::Datacenter);
        assert_eq!(CostBasis::default().sale_stat(), None);
        assert_eq!(RevenueMetric::default().sale_stat(), None);
    }

    #[test]
    fn world_min_token_no_longer_parses() {
        // `revenue=world-min` is handled by the recipe analyzer's URL
        // compat mapping, not by the enum.
        assert!("world-min".parse::<RevenueMetric>().is_err());
    }

    #[test]
    fn override_listings_prefers_the_override_where_present() {
        let base = listings(&[(1, false, 100, 7), (2, false, 200, 7)]);
        let world = listings(&[(1, false, 150, 42)]);
        let merged = override_listings(&base, &world);
        // Item 1: the sell world's own (higher) listing wins — that is the
        // price you'd actually list at.
        assert_eq!(merged.find_matching_listings(1).lowest_gil(), Some(150));
        // Item 2: no listing on the sell world — fall back to the base map
        // so the row isn't dropped as unpriceable.
        assert_eq!(merged.find_matching_listings(2).lowest_gil(), Some(200));
    }

    #[test]
    fn overlay_overrides_listed_items_with_the_chosen_stat() {
        let base = listings(&[(1, false, 500, 42)]);
        let overlaid = overlay_sale_stats(
            &base,
            &stats(&[(1, false, 100, 300, 350)]),
            SaleStat::Median,
        );
        let entry = overlaid
            .map
            .get(&CheapestListingMapKey {
                item_id: 1,
                hq: false,
            })
            .unwrap();
        assert_eq!(entry.price, 300);
        // The listing's cheapest-world tag survives the re-price.
        assert_eq!(entry.world_id, 42);

        let min = overlay_sale_stats(&base, &stats(&[(1, false, 100, 300, 350)]), SaleStat::Min);
        assert_eq!(min.find_matching_listings(1).lowest_gil(), Some(100));
        let avg = overlay_sale_stats(&base, &stats(&[(1, false, 100, 300, 350)]), SaleStat::Avg);
        assert_eq!(avg.find_matching_listings(1).lowest_gil(), Some(350));
    }

    #[test]
    fn items_without_sales_keep_their_listing_price() {
        let base = listings(&[(1, false, 500, 42), (2, false, 900, 42)]);
        let overlaid = overlay_sale_stats(
            &base,
            &stats(&[(1, false, 100, 300, 350)]),
            SaleStat::Median,
        );
        // Item 2 had no sales in the window — the current listing must
        // survive rather than the item pricing at 0.
        assert_eq!(overlaid.find_matching_listings(2).lowest_gil(), Some(900));
    }

    #[test]
    fn items_with_sales_but_no_listing_gain_an_entry() {
        let base = listings(&[]);
        let overlaid =
            overlay_sale_stats(&base, &stats(&[(3, true, 100, 300, 350)]), SaleStat::Median);
        let summary = overlaid.find_matching_listings(3);
        assert_eq!(summary.price_preferring_hq(), Some(300));
        assert_eq!(summary.hq.unwrap().world_id, 0);
    }

    #[test]
    fn zero_priced_stats_are_ignored() {
        let base = listings(&[(1, false, 500, 42)]);
        let overlaid = overlay_sale_stats(&base, &stats(&[(1, false, 0, 0, 0)]), SaleStat::Median);
        assert_eq!(overlaid.find_matching_listings(1).lowest_gil(), Some(500));
    }

    #[test]
    fn hq_and_nq_rows_stay_separate() {
        let base = listings(&[(1, false, 500, 42), (1, true, 800, 43)]);
        let overlaid = overlay_sale_stats(
            &base,
            &stats(&[(1, false, 100, 300, 350), (1, true, 200, 600, 650)]),
            SaleStat::Median,
        );
        let summary = overlaid.find_matching_listings(1);
        assert_eq!(summary.lq.unwrap().price, 300);
        assert_eq!(summary.hq.unwrap().price, 600);
    }
}
