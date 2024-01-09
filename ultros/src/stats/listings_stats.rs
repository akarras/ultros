use std::collections::{HashMap, BTreeMap};

use chrono::{Date, Utc};
use universalis::ItemId;

struct SmallData {
    number_sold: i32,
    price: i32,
}

struct DayDataPoint {
    data_points: Vec<SmallData>,
}

struct HistoricalStatistics(Vec<(ItemId), Vec<DayDataPoint>>);

struct Stat {
    percentile: u8
}

struct Statistics<T>(Vec<(T, Stat)>);

#[cfg(test)]
mod test {
    use chrono::NaiveDateTime;

    use crate::entity::active_listing;

    #[test]
    fn test_listing_stats() {
        let listings = [
            active_listing::Model {
                id: 1,
                world_id: 99,
                item_id: 30,
                retainer_id: 1,
                price_per_unit: 40,
                quantity: 50,
                hq: true,
                timestamp: NaiveDateTime::default(),
            },
            active_listing::Model {
                id: 1,
                world_id: 99,
                item_id: 30,
                retainer_id: 1,
                price_per_unit: 45,
                quantity: 50,
                hq: true,
                timestamp: NaiveDateTime::default(),
            },
            active_listing::Model {
                id: 1,
                world_id: 99,
                item_id: 30,
                retainer_id: 1,
                price_per_unit: 42,
                quantity: 50,
                hq: true,
                timestamp: NaiveDateTime::default(),
            },
            active_listing::Model {
                id: 1,
                world_id: 99,
                item_id: 30,
                retainer_id: 1,
                price_per_unit: 99,
                quantity: 50,
                hq: true,
                timestamp: NaiveDateTime::default(),
            },
        ];
        let mut list : Vec<_> = listings.iter().collect();
        let stats = ListingStats::calculate_stats(&mut list);
    }
}
