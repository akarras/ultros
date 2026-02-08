use std::{
    cmp::Reverse,
    collections::{BTreeMap, btree_map::Entry},
    fmt::Display,
};

use chrono::{Duration, NaiveDateTime};
use poise::serenity_prelude::Timestamp;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use ultros_api_types::{ActiveListing, SaleHistory as ApiSaleHistory};
use ultros_db::{
    UltrosDb,
    entity::{active_listing, sale_history},
    sales::AbbreviatedSaleData,
    world_cache::{AnySelector, WorldCache},
};
use universalis::{ItemId, WorldId};

pub const SALE_HISTORY_SIZE: usize = 6;

#[derive(
    Hash,
    Eq,
    PartialEq,
    PartialOrd,
    Ord,
    Debug,
    Copy,
    Clone,
    Serialize,
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
)]
#[archive(check_bytes)]
pub(crate) struct ItemKey {
    pub(crate) item_id: i32,
    pub(crate) hq: bool,
}

impl Ord for ArchivedItemKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.item_id
            .cmp(&other.item_id)
            .then_with(|| self.hq.cmp(&other.hq))
    }
}

impl PartialOrd for ArchivedItemKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ArchivedItemKey {
    fn eq(&self, other: &Self) -> bool {
        self.item_id == other.item_id && self.hq == other.hq
    }
}

impl Eq for ArchivedItemKey {}

impl From<&active_listing::Model> for ItemKey {
    fn from(model: &active_listing::Model) -> Self {
        let active_listing::Model { item_id, hq, .. } = *model;
        Self { item_id, hq }
    }
}

impl From<&ActiveListing> for ItemKey {
    fn from(value: &ActiveListing) -> Self {
        let ActiveListing { item_id, hq, .. } = *value;
        Self { item_id, hq }
    }
}

impl From<&sale_history::Model> for ItemKey {
    fn from(model: &sale_history::Model) -> Self {
        let sale_history::Model {
            sold_item_id, hq, ..
        } = *model;
        Self {
            item_id: sold_item_id,
            hq,
        }
    }
}

impl From<&ApiSaleHistory> for ItemKey {
    fn from(value: &ApiSaleHistory) -> Self {
        Self {
            item_id: value.sold_item_id,
            hq: value.hq,
        }
    }
}

impl From<&AbbreviatedSaleData> for ItemKey {
    fn from(sale_data: &AbbreviatedSaleData) -> Self {
        Self {
            item_id: sale_data.sold_item_id,
            hq: sale_data.hq,
        }
    }
}

impl From<&ultros_db::listings::ListingSummary> for ItemKey {
    fn from(sum: &ultros_db::listings::ListingSummary) -> Self {
        Self {
            item_id: sum.item_id,
            hq: sum.hq,
        }
    }
}

#[derive(
    Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Copy, Archive, RkyvDeserialize, RkyvSerialize,
)]
#[archive(check_bytes)]
pub(crate) struct SaleSummary {
    pub(crate) price_per_item: i32,
    pub(crate) sale_date: NaiveDateTime,
}

impl From<&AbbreviatedSaleData> for SaleSummary {
    fn from(sale: &AbbreviatedSaleData) -> Self {
        Self {
            sale_date: sale.sold_date,
            price_per_item: sale.price_per_item,
        }
    }
}

impl From<&ultros_db::entity::sale_history::Model> for SaleSummary {
    fn from(sale: &ultros_db::entity::sale_history::Model) -> Self {
        Self {
            sale_date: sale.sold_date,
            price_per_item: sale.price_per_item,
        }
    }
}

impl From<&ApiSaleHistory> for SaleSummary {
    fn from(value: &ApiSaleHistory) -> Self {
        Self {
            price_per_item: value.price_per_item,
            sale_date: value.sold_date,
        }
    }
}

#[derive(Debug, Default, Clone, Archive, RkyvDeserialize, RkyvSerialize)]
#[archive(check_bytes)]
pub(crate) struct SaleHistory {
    pub(crate) item_map: BTreeMap<ItemKey, arrayvec::ArrayVec<SaleSummary, SALE_HISTORY_SIZE>>,
}

impl SaleHistory {
    pub(crate) fn add_sale<'a, T>(&mut self, sale: &'a T)
    where
        &'a T: Into<SaleSummary> + Into<ItemKey>,
    {
        let entries = self.item_map.entry(sale.into()).or_default();
        let sale: SaleSummary = sale.into();
        if entries.len() == SALE_HISTORY_SIZE {
            let last_entry = entries.last().expect("We just checked len");
            if last_entry.sale_date < sale.sale_date {
                let _ = entries.pop();
                entries.push(sale);
            }
        } else {
            entries.push(sale);
        }
        entries.sort_by_key(|sale| Reverse(sale.sale_date));
    }
}

#[derive(Debug, Copy, Clone, Eq, Serialize, Archive, RkyvDeserialize, RkyvSerialize)]
#[archive(check_bytes)]
pub(crate) struct CheapestListingValue {
    pub(crate) price: i32,
    pub(crate) world_id: i32,
}

impl From<&ultros_db::entity::active_listing::Model> for CheapestListingValue {
    fn from(from: &ultros_db::entity::active_listing::Model) -> Self {
        Self {
            price: from.price_per_unit,
            world_id: from.world_id,
        }
    }
}

impl From<&ActiveListing> for CheapestListingValue {
    fn from(
        ActiveListing {
            world_id,
            price_per_unit,
            ..
        }: &ActiveListing,
    ) -> Self {
        Self {
            price: *price_per_unit,
            world_id: *world_id,
        }
    }
}

impl From<&ultros_db::listings::ListingSummary> for CheapestListingValue {
    fn from(from: &ultros_db::listings::ListingSummary) -> Self {
        Self {
            price: from.price_per_unit,
            world_id: from.world_id,
        }
    }
}

impl PartialEq for CheapestListingValue {
    fn eq(&self, other: &Self) -> bool {
        self.price == other.price
    }
}

impl PartialOrd for CheapestListingValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CheapestListingValue {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.price.cmp(&other.price)
    }
}

#[derive(Debug, Default, Archive, RkyvDeserialize, RkyvSerialize, Clone)]
#[archive(check_bytes)]
pub(crate) struct CheapestListings {
    pub(crate) item_map: BTreeMap<ItemKey, CheapestListingValue>,
}

impl CheapestListings {
    pub(crate) fn add_listing<'a, T>(&mut self, listing: &'a T)
    where
        &'a T: Into<CheapestListingValue> + Into<ItemKey>,
    {
        let cheapest_listing = listing.into();
        let entry = self
            .item_map
            .entry(listing.into())
            .or_insert(cheapest_listing);
        *entry = cheapest_listing.min(*entry);
    }

    pub(crate) async fn remove_listing(
        &mut self,
        listing: &ActiveListing,
        id: AnySelector,
        world_cache: &WorldCache,
        ultros_db: &UltrosDb,
    ) {
        // if this was the cheapest listing we need to ask the database for the new cheapest item
        let key = listing.into();
        match self.item_map.entry(key) {
            Entry::Occupied(entry) => {
                // only remove a listing if we see a lower price
                if listing.price_per_unit <= entry.get().price {
                    entry.remove();
                    let worlds = world_cache
                        .lookup_selector(&id)
                        .map(|r| world_cache.get_all_worlds_in(&r))
                        .ok()
                        .flatten()
                        .expect("Should have worlds");
                    if let Ok(listings) = ultros_db
                        .get_multiple_listings_for_worlds_hq_sensitive(
                            worlds.iter().map(|w| WorldId(*w)),
                            [ItemId(listing.item_id)].into_iter(),
                            key.hq,
                            1,
                        )
                        .await
                    {
                        for db_listing in &listings {
                            if key == ItemKey::from(db_listing) {
                                self.add_listing(db_listing);
                            }
                        }
                    }
                }
            }
            Entry::Vacant(_) => {}
        }
    }
}

#[derive(Archive, RkyvDeserialize, RkyvSerialize)]
#[archive(check_bytes)]
pub(crate) struct AnalyzerState {
    pub(crate) recent_sale_history: BTreeMap<i32, SaleHistory>,
    pub(crate) cheapest_items: BTreeMap<AnySelector, CheapestListings>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SoldAmount(pub(crate) u8);

impl Display for SoldAmount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0 >= SALE_HISTORY_SIZE as u8 {
            write!(f, "{}+", SALE_HISTORY_SIZE)
        } else {
            write!(f, "{}", self.0)
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub(crate) enum SoldWithin {
    NoSales,
    Today(SoldAmount),
    Week(SoldAmount),
    Month(SoldAmount),
    Year(SoldAmount),
    YearsAgo(u8, SoldAmount),
}

impl PartialOrd for SoldWithin {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (SoldWithin::NoSales, SoldWithin::NoSales) => Some(std::cmp::Ordering::Equal),
            (SoldWithin::NoSales, _) => None,
            (_, SoldWithin::NoSales) => None,
            (SoldWithin::Today(a), SoldWithin::Today(b)) => Some(b.cmp(a)),
            (SoldWithin::Today(_), _) => Some(std::cmp::Ordering::Less),
            (SoldWithin::Week(_), SoldWithin::Today(_)) => Some(std::cmp::Ordering::Greater),
            (SoldWithin::Week(a), SoldWithin::Week(b)) => Some(b.cmp(a)),
            (SoldWithin::Week(_), _) => Some(std::cmp::Ordering::Less),
            (SoldWithin::Month(_), SoldWithin::Today(_) | SoldWithin::Week(_)) => {
                Some(std::cmp::Ordering::Greater)
            }
            (SoldWithin::Month(a), SoldWithin::Month(b)) => Some(b.cmp(a)),
            (SoldWithin::Month(_), SoldWithin::Year(_) | SoldWithin::YearsAgo(_, _)) => {
                Some(std::cmp::Ordering::Less)
            }
            (
                SoldWithin::Year(_),
                SoldWithin::Today(_) | SoldWithin::Week(_) | SoldWithin::Month(_),
            ) => Some(std::cmp::Ordering::Greater),
            (SoldWithin::Year(a), SoldWithin::Year(b)) => Some(b.cmp(a)),
            (SoldWithin::Year(_), SoldWithin::YearsAgo(_, _)) => Some(std::cmp::Ordering::Less),
            (SoldWithin::YearsAgo(a, aa), SoldWithin::YearsAgo(b, bb)) => {
                Some(a.cmp(b).then_with(|| aa.cmp(bb)))
            }
            (SoldWithin::YearsAgo(_, _), _) => Some(std::cmp::Ordering::Greater),
        }
    }
}

impl Display for SoldWithin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SoldWithin::NoSales => write!(f, "No sales"),
            SoldWithin::Today(d) => write!(f, "{d} sold today"),
            SoldWithin::Week(w) => write!(f, "{w} sold this week"),
            SoldWithin::Month(m) => write!(f, "{m} sold this month"),
            SoldWithin::Year(y) => write!(f, "{y} sold this year"),
            SoldWithin::YearsAgo(i, y) => write!(f, "{y} sold {i} years ago"),
        }
    }
}

impl From<&SoldWithin> for Duration {
    fn from(sold: &SoldWithin) -> Self {
        match sold {
            SoldWithin::NoSales => Duration::days(0),
            SoldWithin::Today(_) => Duration::days(1),
            SoldWithin::Week(_) => Duration::weeks(1),
            SoldWithin::Month(_) => Duration::weeks(4),
            SoldWithin::Year(_) => Duration::weeks(52),
            SoldWithin::YearsAgo(year, _) => Duration::weeks((*year as i64) * 52),
        }
    }
}

impl SoldWithin {
    pub(crate) fn calculate<'a>(
        iter: impl IntoIterator<Item = &'a SaleSummary>,
        now: NaiveDateTime,
    ) -> Self {
        let mut iter = iter.into_iter().peekable();
        let first_sale = match iter.peek() {
            Some(s) => s,
            None => return SoldWithin::NoSales,
        };
        let duration_since = now.signed_duration_since(first_sale.sale_date);
        enum SaleMarker {
            Today,
            Week,
            Month,
            Year,
            YearsAgo(i64),
        }
        let (marker, end_date) = if duration_since.num_days() < 1 {
            (SaleMarker::Today, now.checked_sub_signed(Duration::days(1)))
        } else if duration_since.num_weeks() < 1 {
            (SaleMarker::Week, now.checked_sub_signed(Duration::weeks(1)))
        } else if duration_since.num_weeks() < 4 {
            (
                SaleMarker::Month,
                now.checked_sub_signed(Duration::weeks(4)),
            )
        } else if duration_since.num_weeks() < 52 {
            (
                SaleMarker::Year,
                now.checked_sub_signed(Duration::weeks(52)),
            )
        } else {
            let years = duration_since.num_weeks() / 52;
            (
                SaleMarker::YearsAgo(years),
                now.checked_sub_signed(Duration::weeks((years + 1) * 52)),
            )
        };
        let end_date = match end_date {
            Some(d) => d,
            None => return SoldWithin::NoSales,
        };
        let sold_amount = iter.filter(|sale| sale.sale_date.gt(&end_date)).count() as u8;
        let sold_amount = SoldAmount(sold_amount);
        match marker {
            SaleMarker::Today => SoldWithin::Today(sold_amount),
            SaleMarker::Week => SoldWithin::Week(sold_amount),
            SaleMarker::Month => SoldWithin::Month(sold_amount),
            SaleMarker::Year => SoldWithin::Year(sold_amount),
            SaleMarker::YearsAgo(year) => SoldWithin::YearsAgo(year as u8, sold_amount),
        }
    }
}

impl<'a> FromIterator<&'a SaleSummary> for SoldWithin {
    fn from_iter<T: IntoIterator<Item = &'a SaleSummary>>(iter: T) -> Self {
        SoldWithin::calculate(iter, Timestamp::now().naive_utc())
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResaleStats {
    pub(crate) profit: i32,
    pub(crate) item_id: i32,
    pub(crate) sold_within: SoldWithin,
    pub(crate) return_on_investment: f32,
    pub(crate) world_id: i32,
}

#[derive(Default)]
pub(crate) struct ResaleOptions {
    pub(crate) minimum_profit: Option<i32>,
    pub(crate) filter_world: Option<i32>,
    pub(crate) filter_datacenter: Option<i32>,
    pub(crate) filter_sale: Option<SoldWithin>,
}

#[cfg(test)]
mod test {
    use chrono::{Duration, Utc};
    use ultros_db::sales::AbbreviatedSaleData;

    use crate::analyzer_service::types::ItemKey;

    use super::{SaleHistory, SaleSummary, SoldAmount, SoldWithin};

    #[test]
    fn test_sale_history_sort() {
        let mut sale_history = SaleHistory::default();
        for i in 0..10 {
            sale_history.add_sale(&AbbreviatedSaleData {
                sold_item_id: 101,
                hq: true,
                price_per_item: i,
                sold_date: Utc::now()
                    .naive_utc()
                    .checked_add_signed(Duration::seconds(i as i64))
                    .unwrap(),
                world_id: 0,
            });
        }
        let map = sale_history
            .item_map
            .get(&ItemKey {
                item_id: 101,
                hq: true,
            })
            .unwrap();
        assert_eq!(map[0].price_per_item, 9);
        assert_eq!(map[1].price_per_item, 8);
    }

    #[test]
    fn test_sold_within_calculation() {
        let now = Utc::now().naive_utc();

        // Helper to create a SaleSummary
        let make_sale = |offset_duration: Duration| -> SaleSummary {
            SaleSummary {
                price_per_item: 100,
                sale_date: now + offset_duration,
            }
        };

        // Case 1: No sales
        let sales: Vec<SaleSummary> = vec![];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::NoSales,
            "Empty sales should result in NoSales"
        );

        // Case 2: Sold Today
        // Sale just happened (0 seconds ago)
        let sales = vec![make_sale(Duration::seconds(0))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Today(SoldAmount(1)),
            "Sale at now should be SoldWithin::Today"
        );

        // Sale 23 hours ago is still "Today" if we consider < 24h as logic (which num_days() < 1 implies, wait check impl)
        // logic: duration_since.num_days() < 1. duration_since is now - first_sale.
        // if first_sale is 23h ago, duration_since is 23h. num_days() is 0. So it is Today.
        let sales = vec![make_sale(-Duration::hours(23))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Today(SoldAmount(1)),
            "Sale 23 hours ago should be SoldWithin::Today"
        );

        // Case 3: Sold This Week
        // Sale 25 hours ago. num_days() is 1. num_weeks() is 0. So Week.
        let sales = vec![make_sale(-Duration::hours(25))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Week(SoldAmount(1)),
            "Sale 25 hours ago should be SoldWithin::Week"
        );

        // Sale 6 days ago. num_days() is 6. num_weeks() is 0. So Week.
        let sales = vec![make_sale(-Duration::days(6))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Week(SoldAmount(1)),
            "Sale 6 days ago should be SoldWithin::Week"
        );

        // Case 4: Sold This Month
        // Sale 8 days ago. num_weeks() is 1. So Month. (logic: < 4 weeks is Month)
        let sales = vec![make_sale(-Duration::days(8))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Month(SoldAmount(1)),
            "Sale 8 days ago should be SoldWithin::Month"
        );

        // Sale 3 weeks ago. num_weeks() is 3. So Month.
        let sales = vec![make_sale(-Duration::weeks(3))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Month(SoldAmount(1)),
            "Sale 3 weeks ago should be SoldWithin::Month"
        );

        // Case 5: Sold This Year
        // Sale 5 weeks ago. num_weeks() is 5. So Year. (logic: < 52 weeks is Year)
        let sales = vec![make_sale(-Duration::weeks(5))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Year(SoldAmount(1)),
            "Sale 5 weeks ago should be SoldWithin::Year"
        );

        // Case 6: Sold Years Ago
        // Sale 53 weeks ago. num_weeks() is 53. 53/52 = 1. So YearsAgo(1).
        let sales = vec![make_sale(-Duration::weeks(53))];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::YearsAgo(1, SoldAmount(1)),
            "Sale 53 weeks ago should be SoldWithin::YearsAgo(1)"
        );

        // Case 7: Multiple sales count
        // 3 sales today
        let sales = vec![
            make_sale(-Duration::hours(1)),
            make_sale(-Duration::hours(2)),
            make_sale(-Duration::hours(3)),
        ];
        // The logic uses the first sale (from peek) to determine the "marker".
        // The list is usually sorted by date desc?
        // Wait, SaleHistory.add_sale sorts by date desc (Reverse).
        // Let's assume input is sorted desc (newest first).
        // But `FromIterator` impl takes an iterator. It peeks the first one.
        // In `SoldWithin::calculate`, `iter` is just an iterator.
        // It peeks to find the *most recent* sale to determine the "bucket" (Today/Week/etc).
        // Then it counts how many sales fit in that bucket.
        //
        // Logic detail:
        // marker determined by `now - first_sale`.
        // end_date determined by marker.
        // sold_amount = iter.filter(|sale| sale.sale_date.gt(&end_date)).count()
        //
        // If sales are sorted desc:
        // 1h ago, 2h ago, 3h ago.
        // first = 1h ago. Marker = Today. end_date = now - 1 day.
        // All 3 are > end_date. Count should be 3.
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Today(SoldAmount(3)),
            "3 sales today should be counted correctly"
        );

        // Case 8: Mixed sales
        // 1 sale today, 1 sale yesterday (Week bucket).
        // If sorted desc: first is Today. Marker = Today. end_date = now - 1 day.
        // Today sale > end_date. Yesterday sale (say 25h ago) < end_date.
        // Count should be 1.
        let sales = vec![
            make_sale(-Duration::hours(1)),
            make_sale(-Duration::hours(25)),
        ];
        assert_eq!(
            SoldWithin::calculate(&sales, now),
            SoldWithin::Today(SoldAmount(1)),
            "Should only count sales within the 'Today' window"
        );
    }
}
