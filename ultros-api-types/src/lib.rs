pub mod alert;
pub mod bootstrap;
pub mod cheapest_listings;
mod ffxiv_character;
pub mod icon_size;
pub mod item_stats;
pub mod list;
mod listings;
pub mod market_heat;
pub mod market_pulse;
pub mod recent_sales;
pub mod resale_quality;
pub mod result;
pub mod retainer;
mod sale_history;
pub mod search;
pub mod sparklines;
pub mod trends;
pub mod user;
pub mod websocket;
pub mod world;
pub mod world_helper;

pub use ffxiv_character::*;
pub use listings::ActiveListing;
pub use retainer::Retainer;
pub use sale_history::{CompactSale, ExtendedSaleHistory, SaleHistory};

use crate::websocket::{EventType, ListingEventData, SaleEventData};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentlyShownItem {
    pub listings: Vec<(ActiveListing, Retainer)>,
    pub sales: Vec<SaleHistory>,
}

impl CurrentlyShownItem {
    pub fn apply_listing_event(&mut self, target_item_id: i32, event: EventType<ListingEventData>) {
        match event {
            EventType::Added(event) | EventType::Updated(event) => {
                if event.item_id != target_item_id {
                    return;
                }
                self.upsert_listings(event.listings);
            }
            EventType::Removed(event) => {
                if event.item_id != target_item_id {
                    return;
                }
                self.remove_listings(event.listings);
            }
        }
        self.listings
            .sort_by_key(|(listing, _)| (listing.hq, listing.price_per_unit));
    }

    fn upsert_listings(&mut self, listings: Vec<(ActiveListing, Retainer)>) {
        for incoming in listings {
            self.listings
                .retain(|(listing, _)| listing.id != incoming.0.id);
            self.listings.push(incoming);
        }
    }

    fn remove_listings(&mut self, listings: Vec<(ActiveListing, Retainer)>) {
        for (removed, _) in listings {
            self.listings
                .retain(|(listing, _)| listing.id != removed.id);
        }
    }

    pub fn apply_sales_event(&mut self, target_item_id: i32, event: EventType<SaleEventData>) {
        match event {
            EventType::Added(event) | EventType::Updated(event) => {
                self.upsert_sales(
                    event
                        .sales
                        .into_iter()
                        .filter(|(sale, _)| sale.sold_item_id == target_item_id)
                        .map(|(sale, _)| sale)
                        .collect::<Vec<_>>(),
                );
            }
            EventType::Removed(event) => {
                for (removed, _) in event.sales {
                    if removed.sold_item_id == target_item_id {
                        self.sales.retain(|sale| sale.id != removed.id);
                    }
                }
            }
        }
        self.sales
            .sort_by_key(|sale| std::cmp::Reverse(sale.sold_date));
        self.sales.truncate(200);
    }

    fn upsert_sales(&mut self, sales: Vec<SaleHistory>) {
        for incoming in sales {
            self.sales.retain(|sale| sale.id != incoming.id);
            self.sales.push(incoming);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::websocket::{EventType, ListingEventData, SaleEventData};
    use chrono::NaiveDateTime;

    fn test_listing(id: i32, item_id: i32, price: i32) -> (ActiveListing, Retainer) {
        (
            ActiveListing {
                id,
                world_id: 1,
                item_id,
                retainer_id: 1,
                price_per_unit: price,
                quantity: 1,
                hq: false,
                timestamp: NaiveDateTime::default(),
            },
            Retainer {
                id: 1,
                world_id: 1,
                name: "Retainer".to_string(),
                retainer_city_id: 1,
            },
        )
    }

    fn test_sale(
        id: i32,
        item_id: i32,
        price: i32,
        sold_date: NaiveDateTime,
    ) -> (SaleHistory, UnknownCharacter) {
        (
            SaleHistory {
                id,
                quantity: 1,
                price_per_item: price,
                buying_character_id: 1,
                hq: false,
                sold_item_id: item_id,
                sold_date,
                world_id: 1,
                buyer_name: Some("Buyer".to_string()),
            },
            UnknownCharacter {
                id: 1,
                name: "Buyer".to_string(),
            },
        )
    }

    #[test]
    fn test_apply_listing_event_add() {
        let mut data = CurrentlyShownItem {
            listings: vec![],
            sales: vec![],
        };
        let event = EventType::Added(ListingEventData {
            item_id: 1,
            world_id: 1,
            listings: vec![test_listing(1, 1, 100)],
        });

        data.apply_listing_event(1, event);
        assert_eq!(data.listings.len(), 1);
        assert_eq!(data.listings[0].0.id, 1);
    }

    #[test]
    fn test_apply_listing_event_update() {
        let mut data = CurrentlyShownItem {
            listings: vec![test_listing(1, 1, 100)],
            sales: vec![],
        };
        let event = EventType::Updated(ListingEventData {
            item_id: 1,
            world_id: 1,
            listings: vec![test_listing(1, 1, 150)],
        });

        data.apply_listing_event(1, event);
        assert_eq!(data.listings.len(), 1);
        assert_eq!(data.listings[0].0.price_per_unit, 150);
    }

    #[test]
    fn test_apply_listing_event_remove() {
        let mut data = CurrentlyShownItem {
            listings: vec![test_listing(1, 1, 100)],
            sales: vec![],
        };
        let event = EventType::Removed(ListingEventData {
            item_id: 1,
            world_id: 1,
            listings: vec![test_listing(1, 1, 100)],
        });

        data.apply_listing_event(1, event);
        assert_eq!(data.listings.len(), 0);
    }

    #[test]
    fn test_apply_listing_event_wrong_item_id() {
        let mut data = CurrentlyShownItem {
            listings: vec![test_listing(1, 1, 100)],
            sales: vec![],
        };
        let event = EventType::Updated(ListingEventData {
            item_id: 2,
            world_id: 1,
            listings: vec![test_listing(1, 2, 150)],
        });

        data.apply_listing_event(1, event);
        // Should not change
        assert_eq!(data.listings.len(), 1);
        assert_eq!(data.listings[0].0.item_id, 1);
        assert_eq!(data.listings[0].0.price_per_unit, 100);
    }

    #[test]
    fn test_apply_sales_event_add() {
        let mut data = CurrentlyShownItem {
            listings: vec![],
            sales: vec![],
        };
        let event = EventType::Added(SaleEventData {
            sales: vec![test_sale(1, 1, 100, NaiveDateTime::default())],
        });

        data.apply_sales_event(1, event);
        assert_eq!(data.sales.len(), 1);
        assert_eq!(data.sales[0].id, 1);
    }

    #[test]
    fn test_apply_sales_event_update() {
        let (sale, _char) = test_sale(1, 1, 100, NaiveDateTime::default());
        let mut data = CurrentlyShownItem {
            listings: vec![],
            sales: vec![sale],
        };
        let event = EventType::Updated(SaleEventData {
            sales: vec![test_sale(1, 1, 150, NaiveDateTime::default())],
        });

        data.apply_sales_event(1, event);
        assert_eq!(data.sales.len(), 1);
        assert_eq!(data.sales[0].price_per_item, 150);
    }

    #[test]
    fn test_apply_sales_event_remove() {
        let (sale, _char) = test_sale(1, 1, 100, NaiveDateTime::default());
        let mut data = CurrentlyShownItem {
            listings: vec![],
            sales: vec![sale],
        };
        let event = EventType::Removed(SaleEventData {
            sales: vec![test_sale(1, 1, 100, NaiveDateTime::default())],
        });

        data.apply_sales_event(1, event);
        assert_eq!(data.sales.len(), 0);
    }

    #[test]
    fn test_apply_sales_event_wrong_item_id() {
        let (sale, _char) = test_sale(1, 1, 100, NaiveDateTime::default());
        let mut data = CurrentlyShownItem {
            listings: vec![],
            sales: vec![sale],
        };
        // Add event for wrong item
        let event = EventType::Added(SaleEventData {
            sales: vec![test_sale(2, 2, 150, NaiveDateTime::default())],
        });
        data.apply_sales_event(1, event);
        assert_eq!(data.sales.len(), 1);
        assert_eq!(data.sales[0].price_per_item, 100);

        // Remove event for wrong item
        let event = EventType::Removed(SaleEventData {
            sales: vec![test_sale(1, 2, 100, NaiveDateTime::default())],
        });
        data.apply_sales_event(1, event);
        assert_eq!(data.sales.len(), 1);
    }

    #[test]
    fn test_apply_sales_event_ordering() {
        let mut data = CurrentlyShownItem {
            listings: vec![],
            sales: vec![],
        };
        let date_early = NaiveDateTime::from_timestamp_opt(1000, 0).unwrap();
        let date_late = NaiveDateTime::from_timestamp_opt(2000, 0).unwrap();

        let event = EventType::Added(SaleEventData {
            sales: vec![
                test_sale(1, 1, 100, date_early),
                test_sale(2, 1, 200, date_late),
            ],
        });

        data.apply_sales_event(1, event);
        assert_eq!(data.sales.len(), 2);
        assert_eq!(data.sales[0].id, 2); // newest first
        assert_eq!(data.sales[1].id, 1);
    }

    #[test]
    fn test_apply_sales_event_truncation() {
        let mut data = CurrentlyShownItem {
            listings: vec![],
            sales: vec![],
        };
        let mut sales = vec![];
        for i in 0..205 {
            sales.push(test_sale(
                i,
                1,
                100,
                NaiveDateTime::from_timestamp_opt(i as i64, 0).unwrap(),
            ));
        }

        let event = EventType::Added(SaleEventData { sales });

        data.apply_sales_event(1, event);
        assert_eq!(data.sales.len(), 200);
        assert_eq!(data.sales[0].id, 204); // newest first
    }
}
