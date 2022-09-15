use crate::entity::active_listing::Model as Listing;

#[derive(Debug)]
pub struct ListingStat {
    /// Numeric percentile representation. Ranges from 0-99
    pub percentile: i8,
}

pub struct ListingStats<'a> {
    pub listings: Vec<(ListingStat, &'a Listing)>,
}

impl<'a> ListingStats<'a> {
    pub fn calculate_stats(listings: &mut [&'a Listing]) -> Self {
        listings.sort_by(|a, b| {
            a.price_per_unit
                .cmp(&b.price_per_unit)
                .then_with(|| a.quantity.cmp(&b.quantity))
        });
        let total = listings.len();
        let listings: Vec<_> = listings
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let percentile = (i as f64 / total as f64 * 100.0) as i8;
                (ListingStat { percentile }, *l)
            })
            .collect();

        Self { listings }
    }
}

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
    }
}
