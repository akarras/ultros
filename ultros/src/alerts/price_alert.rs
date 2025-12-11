use std::{collections::HashMap, fmt, sync::Arc};
use tokio::sync::{
    mpsc::{self, Receiver},
    RwLock,
};
use ultros_api_types::{
    world_helper::{AnySelector as HelperSelector, WorldHelper},
    ActiveListing, Retainer,
};
use ultros_db::world_cache::AnySelector;

use crate::event::EventReceivers;

pub(crate) struct PriceUndercutData {
    pub(crate) item_id: i32,
    pub(crate) price: i32,
}

#[derive(Debug)]
pub(crate) struct PriceAlertDetails {
    /// If price is below this threshold, then send the alert
    pub(crate) price_threshold: i32,
    pub(crate) item_id: i32,
    /// If the price is within this selector, then send the alert
    pub(crate) travel_amount: AnySelector,
    pub(crate) sender: mpsc::Sender<PriceUndercutData>,
}

#[derive(Clone)]
pub(crate) struct PriceAlertService {
    item_map: Arc<RwLock<HashMap<i32, Vec<PriceAlertDetails>>>>,
    world_helper: Arc<WorldHelper>,
}

impl fmt::Debug for PriceAlertService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PriceAlertService")
            .field("item_map", &self.item_map)
            .finish()
    }
}

impl PriceAlertService {
    pub(crate) fn new(event_receiver: EventReceivers, world_helper: Arc<WorldHelper>) -> Self {
        let instance = Self {
            item_map: Default::default(),
            world_helper,
        };
        let i = instance.clone();
        tokio::spawn(async move { i.start_listener(event_receiver).await });
        instance
    }

    async fn start_listener(&self, mut event_receiver: EventReceivers) {
        loop {
            if let Ok(crate::event::EventType::Add(l)) = event_receiver.listings.recv().await {
                self.check_listings(&l.listings).await;
            }
        }
    }

    async fn check_listings(&self, listings: &[(ActiveListing, Retainer)]) {
        let read = self.item_map.read().await;
        // Group listings by item_id to handle batches efficiently
        let mut listings_by_item: HashMap<i32, Vec<&(ActiveListing, Retainer)>> = HashMap::new();
        for listing in listings {
            listings_by_item
                .entry(listing.0.item_id)
                .or_default()
                .push(listing);
        }

        let mut notifications = Vec::new();

        for (item_id, item_listings) in listings_by_item {
            if let Some(alerts) = read.get(&item_id) {
                for alert in alerts {
                    // Optimization: Hoist selector lookup
                    let alert_selector_enum = match alert.travel_amount {
                        AnySelector::World(id) => HelperSelector::World(id),
                        AnySelector::Datacenter(id) => HelperSelector::Datacenter(id),
                        AnySelector::Region(id) => HelperSelector::Region(id),
                    };

                    if let Some(alert_selector) =
                        self.world_helper.lookup_selector(alert_selector_enum)
                    {
                        let lowest_matching_price = item_listings
                            .iter()
                            .filter(|(l, _)| {
                                // Check if the listing's world is within the alert's travel_amount (selector)
                                let listing_world_selector = HelperSelector::World(l.world_id);
                                if let Some(listing_world) =
                                    self.world_helper.lookup_selector(listing_world_selector)
                                {
                                    listing_world.is_in(&alert_selector)
                                } else {
                                    false
                                }
                            })
                            .map(|(l, _)| l.price_per_unit)
                            .filter(|&price| price <= alert.price_threshold)
                            .min();

                        if let Some(price) = lowest_matching_price {
                            notifications.push((alert.sender.clone(), item_id, price));
                        }
                    }
                }
            }
        }
        drop(read);

        for (sender, item_id, price) in notifications {
            let _ = sender.send(PriceUndercutData { item_id, price }).await;
        }
    }

    pub(crate) async fn create_alert(
        &self,
        price_threshold: i32,
        item_id: i32,
        travel_amount: AnySelector,
    ) -> Receiver<PriceUndercutData> {
        let (sender, receiver) = mpsc::channel(10);
        let details = PriceAlertDetails {
            price_threshold,
            item_id,
            travel_amount,
            sender,
        };

        let mut write = self.item_map.write().await;
        let entry = write.entry(item_id).or_default();
        entry.push(details);
        receiver
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use ultros_api_types::{
        world::{Region, World, WorldData},
        world_helper::WorldHelper,
        ActiveListing, Retainer,
    };
    use ultros_db::world_cache::AnySelector;

    use crate::event::create_event_busses;

    use super::PriceAlertService;

    fn create_mock_world_helper() -> WorldHelper {
        let world = World {
            id: 1,
            name: "TestWorld".to_string(),
            datacenter_id: 1,
        };
        let datacenter = ultros_api_types::world::Datacenter {
            id: 1,
            name: "TestDC".to_string(),
            region_id: 1,
            worlds: vec![world.clone()],
        };
        let region = Region {
            id: 1,
            name: "TestRegion".to_string(),
            datacenters: vec![datacenter],
        };
        let world_data = WorldData {
            regions: vec![region],
        };
        WorldHelper::new(world_data)
    }

    #[tokio::test]
    async fn test_price_alert() {
        let (senders, receivers) = create_event_busses();
        let world_helper = Arc::new(create_mock_world_helper());
        let price_alert_service = PriceAlertService::new(receivers, world_helper);

        // Create an alert
        let mut alert_receiver = price_alert_service
            .create_alert(1000, 123, AnySelector::World(1))
            .await;

        // Create a listing that matches
        let listing = ActiveListing {
            id: 1,
            world_id: 1,
            item_id: 123,
            retainer_id: 1,
            price_per_unit: 900,
            quantity: 1,
            hq: false,
            timestamp: chrono::Utc::now().naive_utc(),
        };
        let retainer = Retainer {
            id: 1,
            world_id: 1,
            name: "Retainer".to_string(),
            retainer_city_id: 1,
        };

        // Send the listing
        let event_data =
            ultros_api_types::websocket::ListingEventData {
                item_id: 123,
                world_id: 1,
                listings: vec![(listing, retainer)],
            };
        senders
            .listings
            .send(crate::event::EventType::Add(std::sync::Arc::new(
                event_data,
            )))
            .unwrap();

        // Check if we received the alert
        let alert = tokio::time::timeout(std::time::Duration::from_secs(1), alert_receiver.recv())
            .await
            .expect("Should receive alert")
            .expect("Channel shouldn't be closed");

        assert_eq!(alert.item_id, 123);
        assert_eq!(alert.price, 900);
    }
}
