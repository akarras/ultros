use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{
    RwLock,
    mpsc::{self},
};
use tracing::{error, info};
use ultros_api_types::{
    ActiveListing, Retainer,
    world_helper::{AnySelector, WorldHelper},
};

use crate::event::EventReceivers;

#[derive(Debug, Clone)]
pub(crate) struct PriceUndercutData {
    pub(crate) item_id: i32,
    pub(crate) price: i32,
    pub(crate) world_id: i32,
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

impl std::fmt::Debug for PriceAlertService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
        let i = instance.clone();
        tokio::spawn(async move { i.start_cleanup_task().await });
        instance
    }

    async fn start_listener(&self, mut event_receiver: EventReceivers) {
        loop {
            if let Ok(crate::event::EventType::Add(l)) = event_receiver.listings.recv().await {
                self.check_listings(&l.listings).await;
            }
        }
    }

    async fn start_cleanup_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(600)); // Every 10 minutes
        loop {
            interval.tick().await;
            self.cleanup_closed_alerts().await;
        }
    }

    #[allow(clippy::collapsible_if)]
    async fn check_listings(&self, listings: &[(ActiveListing, Retainer)]) {
        if listings.is_empty() {
            return;
        }

        let map = self.item_map.read().await;

        for (listing, _) in listings {
            if let Some(alerts) = map.get(&listing.item_id) {
                // Hoist world lookup out of the loop
                let listing_world_scope = self
                    .world_helper
                    .lookup_selector(AnySelector::World(listing.world_id));

                if let Some(listing_world) = listing_world_scope {
                    for alert in alerts {
                        // Check price condition
                        if listing.price_per_unit < alert.price_threshold {
                            // Check world condition
                            if let Some(alert_scope) =
                                self.world_helper.lookup_selector(alert.travel_amount)
                            {
                                if listing_world.is_in(&alert_scope) {
                                    // Match! Send alert.
                                    let data = PriceUndercutData {
                                        item_id: listing.item_id,
                                        price: listing.price_per_unit,
                                        world_id: listing.world_id,
                                    };

                                    if alert.sender.send(data).await.is_err() {
                                        // Channel closed.
                                        // Cleanup will handle this later.
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // cleanup version
    pub(crate) async fn cleanup_closed_alerts(&self) {
        let mut map = self.item_map.write().await;
        let before = map.len();
        for (_, alerts) in map.iter_mut() {
            alerts.retain(|a| !a.sender.is_closed());
        }
        // remove empty entries
        map.retain(|_, v| !v.is_empty());
        let after = map.len();
        if before != after {
            info!("Cleaned up closed alerts. Before: {before}, After: {after}");
        }
    }

    pub(crate) async fn create_alert(
        &self,
        price_threshold: i32,
        item_id: i32,
        travel_amount: AnySelector,
        sender: mpsc::Sender<PriceUndercutData>,
    ) {
        let details = PriceAlertDetails {
            price_threshold,
            item_id,
            travel_amount,
            sender,
        };

        let mut write = self.item_map.write().await;
        let entry = write.entry(item_id).or_default();
        entry.push(details);
        info!("Created price alert for item {item_id} @ {price_threshold}");
    }
}

#[cfg(test)]
mod test {
    use crate::event::create_event_busses;

    #[tokio::test]
    async fn test_price_alert() {
        // No-op test just to ensure compilation of the module
        let (_senders, _receivers) = create_event_busses();
    }
}
