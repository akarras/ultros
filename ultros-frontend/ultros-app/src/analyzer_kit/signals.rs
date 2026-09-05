//! Price lookups the pricing core can be generic over, and the layered
//! view that prices from a sale statistic with the listing as fallback
//! without cloning any map.

use std::collections::HashMap;
use std::sync::Arc;

use leptos::prelude::RwSignal;

use ultros_api_types::cheapest_listings::{
    CheapestListingData, CheapestListingMapKey, CheapestListingsMap, PriceSummary,
};
use ultros_api_types::sale_stats::{BulkSaleStats, ItemSaleStats};

use super::formula::SaleStat;

/// The one lookup the pricing core needs. `compute_cost` is generic over
/// it, so a lazily layered view can stand in for a cloned map.
pub trait PriceLookup {
    fn find_matching_listings(&self, item_id: i32) -> PriceSummary;
}

impl PriceLookup for CheapestListingsMap {
    fn find_matching_listings(&self, item_id: i32) -> PriceSummary {
        CheapestListingsMap::find_matching_listings(self, item_id)
    }
}

impl<P: PriceLookup + ?Sized> PriceLookup for &P {
    fn find_matching_listings(&self, item_id: i32) -> PriceSummary {
        (**self).find_matching_listings(item_id)
    }
}

impl<P: PriceLookup + ?Sized> PriceLookup for Arc<P> {
    fn find_matching_listings(&self, item_id: i32) -> PriceSummary {
        (**self).find_matching_listings(item_id)
    }
}

/// Sale statistics keyed by `(item_id, hq)`, built once per payload.
pub type StatsIndex = HashMap<(i32, bool), ItemSaleStats>;

pub fn stats_index(stats: &BulkSaleStats) -> StatsIndex {
    stats
        .stats
        .iter()
        .map(|s| ((s.item_id, s.hq), *s))
        .collect()
}

/// A client-only sale-statistics body, filled by a page `Effect` after the
/// table has rendered: `None` on the server and on the first client paint,
/// `Some(index)` once it lands — an *empty* index if the fetch failed, so
/// cells settle to "—" instead of shimmering forever.
pub type LateStats = RwSignal<Option<Arc<StatsIndex>>>;

/// The statistics row for `(item, quality)`, preferring `prefer_hq` and
/// falling back to the other quality: the rule the pricing pass applies to
/// the 7-day body, so a row's 30-day figures come from the same quality its
/// 7-day ones did.
pub fn stat_row_either(
    index: &StatsIndex,
    item_id: i32,
    prefer_hq: bool,
) -> Option<&ItemSaleStats> {
    index
        .get(&(item_id, prefer_hq))
        .or_else(|| index.get(&(item_id, !prefer_hq)))
}

/// The statistic a signal reads from one row.
pub fn stat_price(row: &ItemSaleStats, stat: SaleStat) -> i32 {
    match stat {
        SaleStat::Min => row.min_price,
        SaleStat::Median => row.median_price,
        SaleStat::Avg => row.avg_price,
    }
}

/// The bare statistic for `(item, hq)`: no listing fallback, `None` when
/// the row is missing or zero. Alternative revenue columns read this so a
/// world with no sale history shows "—" rather than a listing.
pub fn stat_only(index: &StatsIndex, item_id: i32, hq: bool, stat: SaleStat) -> Option<i32> {
    index
        .get(&(item_id, hq))
        .map(|row| stat_price(row, stat))
        .filter(|p| *p > 0)
}

/// The cheaper of the NQ and HQ bare statistics — today's revenue rule
/// (the cheaper quality sells first).
pub fn stat_only_cheapest(index: &StatsIndex, item_id: i32, stat: SaleStat) -> Option<i32> {
    match (
        stat_only(index, item_id, false, stat),
        stat_only(index, item_id, true, stat),
    ) {
        (None, None) => None,
        (Some(a), None) | (None, Some(a)) => Some(a),
        (Some(a), Some(b)) => Some(a.min(b)),
    }
}

/// A price view layered from three sources without copying any of them:
/// the `over` listings win where present (the sell world's own price),
/// `base` listings fill the rest (the buy scope), and when a sale statistic
/// is selected it replaces the price of every `(item, hq)` that has a
/// non-zero stat row — keeping the listing's world so "cheapest world"
/// still means something. Items with a stat but no listing gain an entry
/// with world 0; items with a zero or missing stat keep their listing;
/// nothing is ever priced at 0 because of an absent statistic.
///
/// This is exactly `overlay_sale_stats(&override_listings(base, over), ..)`
/// from `price_basis`, evaluated per lookup.
pub struct SignalView<'a> {
    pub over: Option<&'a CheapestListingsMap>,
    pub base: &'a CheapestListingsMap,
    pub stats: Option<(&'a StatsIndex, SaleStat)>,
}

impl SignalView<'_> {
    fn quality(&self, item_id: i32, hq: bool) -> Option<CheapestListingData> {
        let key = CheapestListingMapKey { item_id, hq };
        let listing = self
            .over
            .and_then(|o| o.map.get(&key).copied())
            .or_else(|| self.base.map.get(&key).copied());
        match self.stats {
            Some((index, stat)) => match index.get(&(item_id, hq)) {
                Some(row) if stat_price(row, stat) > 0 => Some(CheapestListingData {
                    price: stat_price(row, stat),
                    world_id: listing.map(|l| l.world_id).unwrap_or(0),
                }),
                _ => listing,
            },
            None => listing,
        }
    }
}

impl PriceLookup for SignalView<'_> {
    fn find_matching_listings(&self, item_id: i32) -> PriceSummary {
        PriceSummary {
            lq: self.quality(item_id, false),
            hq: self.quality(item_id, true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::price_basis::{overlay_sale_stats, override_listings};
    use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};

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

    fn stat_row(item_id: i32, hq: bool, min_price: i32) -> ItemSaleStats {
        ItemSaleStats {
            item_id,
            hq,
            min_price,
            ..Default::default()
        }
    }

    /// Items 1-5 cover: listed both sides, listed only in base, listed
    /// only in over, stats without any listing, and a zero-priced stat.
    fn fixture() -> (CheapestListingsMap, CheapestListingsMap, BulkSaleStats) {
        let base = listings(&[
            (1, false, 100, 7),
            (1, true, 180, 7),
            (2, false, 200, 7),
            (5, false, 50, 7),
        ]);
        let over = listings(&[(1, false, 150, 42), (3, true, 300, 42)]);
        let stats = stats(&[
            (1, false, 90, 120, 130),
            (2, true, 210, 220, 230),
            (4, false, 10, 20, 30),
            (5, false, 0, 0, 0),
        ]);
        (base, over, stats)
    }

    fn assert_same(view: &SignalView<'_>, oracle: &CheapestListingsMap, item: i32) {
        let got = view.find_matching_listings(item);
        let want = oracle.find_matching_listings(item);
        assert_eq!(got.lq, want.lq, "item {item} lq");
        assert_eq!(got.hq, want.hq, "item {item} hq");
    }

    #[test]
    fn signal_view_matches_override_then_overlay_on_fixture() {
        let (base, over, st) = fixture();
        let index = stats_index(&st);
        for stat in [SaleStat::Min, SaleStat::Median, SaleStat::Avg] {
            let oracle = overlay_sale_stats(&override_listings(&base, &over), &st, stat);
            let view = SignalView {
                over: Some(&over),
                base: &base,
                stats: Some((&index, stat)),
            };
            for item in 1..=6 {
                assert_same(&view, &oracle, item);
            }
            // Cost side: no override layer.
            let oracle = overlay_sale_stats(&base, &st, stat);
            let view = SignalView {
                over: None,
                base: &base,
                stats: Some((&index, stat)),
            };
            for item in 1..=6 {
                assert_same(&view, &oracle, item);
            }
        }
        // Listing signal: override only.
        let oracle = override_listings(&base, &over);
        let view = SignalView {
            over: Some(&over),
            base: &base,
            stats: None,
        };
        for item in 1..=6 {
            assert_same(&view, &oracle, item);
        }
    }

    #[test]
    fn signal_view_never_prices_a_missing_or_zero_stat_at_zero() {
        let (base, _, st) = fixture();
        let index = stats_index(&st);
        let view = SignalView {
            over: None,
            base: &base,
            stats: Some((&index, SaleStat::Median)),
        };
        // Item 5 has a zero-priced stat row: keep the listing.
        assert_eq!(view.find_matching_listings(5).lowest_gil(), Some(50));
        // Item 2 NQ has a listing but no stat row: keep the listing.
        assert_eq!(
            view.find_matching_listings(2).lq.map(|d| d.price),
            Some(200)
        );
        // Item 4 has a stat row but no listing: gains an entry with world 0.
        let four = view.find_matching_listings(4);
        assert_eq!(four.lq.map(|d| (d.price, d.world_id)), Some((20, 0)));
        // Item 6 has nothing.
        assert_eq!(view.find_matching_listings(6).lowest_gil(), None);
    }

    #[test]
    fn stat_priced_entries_keep_the_listing_world() {
        let (base, over, st) = fixture();
        let index = stats_index(&st);
        let view = SignalView {
            over: Some(&over),
            base: &base,
            stats: Some((&index, SaleStat::Min)),
        };
        // Item 1 NQ: stat 90 wins, world from the override layer (42).
        assert_eq!(
            view.find_matching_listings(1).lq,
            Some(CheapestListingData {
                price: 90,
                world_id: 42
            })
        );
    }

    #[test]
    fn arc_and_ref_implement_price_lookup() {
        fn takes<P: PriceLookup + ?Sized>(p: &P) -> Option<i32> {
            p.find_matching_listings(1).lowest_gil()
        }
        let (base, _, _) = fixture();
        let arc = Arc::new(base.clone());
        assert_eq!(takes(&base), Some(100));
        assert_eq!(takes(&arc), Some(100));
        assert_eq!(takes(&&base), Some(100));
    }

    #[test]
    fn stat_only_has_no_fallback() {
        let mut index = StatsIndex::new();
        index.insert(
            (7, false),
            ItemSaleStats {
                item_id: 7,
                hq: false,
                min_price: 90,
                median_price: 100,
                avg_price: 110,
                num_sold: 3,
                ..Default::default()
            },
        );
        index.insert(
            (7, true),
            ItemSaleStats {
                item_id: 7,
                hq: true,
                min_price: 0,
                median_price: 80,
                avg_price: 0,
                num_sold: 1,
                ..Default::default()
            },
        );
        assert_eq!(stat_only(&index, 7, false, SaleStat::Median), Some(100));
        assert_eq!(
            stat_only(&index, 7, true, SaleStat::Min),
            None,
            "a zero stat is no stat"
        );
        assert_eq!(
            stat_only(&index, 8, false, SaleStat::Median),
            None,
            "no row, no number"
        );
        assert_eq!(stat_only_cheapest(&index, 7, SaleStat::Median), Some(80));
        assert_eq!(
            stat_only_cheapest(&index, 7, SaleStat::Avg),
            Some(110),
            "the zero HQ avg is skipped"
        );
        assert_eq!(stat_only_cheapest(&index, 8, SaleStat::Min), None);
    }

    #[test]
    fn stat_row_either_falls_back_to_the_other_quality() {
        let mut index: StatsIndex = StatsIndex::new();
        index.insert((7, false), stat_row(7, false, 100));
        assert_eq!(
            stat_row_either(&index, 7, false).map(|r| r.min_price),
            Some(100)
        );
        // HQ preferred but absent: the NQ row is what actually traded.
        assert_eq!(
            stat_row_either(&index, 7, true).map(|r| r.min_price),
            Some(100)
        );
        index.insert((7, true), stat_row(7, true, 250));
        assert_eq!(
            stat_row_either(&index, 7, true).map(|r| r.min_price),
            Some(250)
        );
        assert_eq!(
            stat_row_either(&index, 7, false).map(|r| r.min_price),
            Some(100)
        );
        assert!(stat_row_either(&index, 8, false).is_none());
    }
}
