use std::{collections::HashMap, sync::Arc};
use tokio::sync::{
    mpsc::{self, Receiver},
    RwLock,
};
use ultros_api_types::{ActiveListing, Retainer};
use ultros_db::world_cache::AnySelector;

use crate::event::EventReceivers;

pub(crate) struct PriceUndercutData {
    pub(crate) item_id: i32,
    pub(crate) undercut_by: i32,
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
    /// The last price we alerted on, to avoid spamming the user for the same price.
    pub(crate) last_alerted_price: Option<i32>,
}

#[allow(dead_code)]
enum PriceAlert {
    PricedLow { price_threshold: i32 },
}

#[derive(Debug, Clone)]
pub(crate) struct PriceAlertService {
    item_map: Arc<RwLock<HashMap<i32, Vec<PriceAlertDetails>>>>,
}

impl PriceAlertService {
    pub(crate) fn new(event_receiver: EventReceivers) -> Self {
        let instance = Self {
            item_map: Default::default(),
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
        if listings.is_empty() {
            return;
        }

        // Group listings by (item_id, world_id) to handle multi-world updates correctly
        // We store the minimum price seen for that item on that world in this batch.
        let mut updates: HashMap<(i32, i32), i32> = HashMap::new();

        for (listing, _) in listings {
            let key = (listing.item_id, listing.world_id);
            let entry = updates.entry(key).or_insert(i32::MAX);
            if listing.price_per_unit < *entry {
                *entry = listing.price_per_unit;
            }
        }

        // We also need a set of item_ids to quickly look up relevant alerts
        // Since we iterate item_map, we can check if item_id is in updates?
        // No, we iterate updates? No, updates might be for items nobody cares about.
        // Better: Iterate item_map for items present in updates.
        // Group updates by item_id -> list of (world_id, price)
        let mut updates_by_item: HashMap<i32, Vec<(i32, i32)>> = HashMap::new();
        for ((item_id, world_id), price) in updates {
            updates_by_item.entry(item_id).or_default().push((world_id, price));
        }

        let mut map = self.item_map.write().await;

        for (item_id, world_updates) in updates_by_item {
            if let Some(alerts) = map.get_mut(&item_id) {
                let mut i = 0;
                while i < alerts.len() {
                    let alert = &mut alerts[i];
                    let mut should_remove = false;
                    let mut matched_price: Option<(i32, i32)> = None; // (price, world_id)

                    // Find the best matching price for this alert from the updates
                    for &(world_id, price) in &world_updates {
                         let matches = match alert.travel_amount {
                            AnySelector::World(w) => w == world_id,
                            // TODO: Support Region and Datacenter selectors.
                            _ => false,
                        };

                        if matches {
                            // If we matched, check if this is the best price so far for this alert
                            if let Some((best_p, _)) = matched_price {
                                if price < best_p {
                                    matched_price = Some((price, world_id));
                                }
                            } else {
                                matched_price = Some((price, world_id));
                            }
                        }
                    }

                    if let Some((best_price, world_id)) = matched_price {
                        if best_price < alert.price_threshold {
                            let mut should_alert = true;
                             // Spam prevention
                            if let Some(last_price) = alert.last_alerted_price {
                                if best_price >= last_price {
                                    should_alert = false;
                                }
                            }

                            if should_alert {
                                // Use try_send to avoid holding the lock
                                match alert.sender.try_send(PriceUndercutData {
                                    item_id,
                                    undercut_by: best_price,
                                    world_id,
                                }) {
                                    Ok(_) => {
                                        alert.last_alerted_price = Some(best_price);
                                    }
                                    Err(mpsc::error::TrySendError::Full(_)) => {
                                        // Channel full, slow client. Drop the alert but keep the listener.
                                        // Ideally we might want to count failures and drop eventually, but simple drop is fine for now.
                                    }
                                    Err(mpsc::error::TrySendError::Closed(_)) => {
                                        // Receiver closed, remove listener.
                                        should_remove = true;
                                    }
                                }
                            }
                        }
                    }

                    if should_remove {
                        alerts.swap_remove(i);
                    } else {
                        i += 1;
                    }
                }
            }
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
            last_alerted_price: None,
        };

        let mut write = self.item_map.write().await;
        let entry = write.entry(item_id).or_default();
        entry.push(details);
        receiver
    }
}

#[cfg(test)]
mod test {
    use crate::event::create_event_busses;

    use super::PriceAlertService;

    #[tokio::test]
    async fn test_price_alert() {
        let (senders, receivers) = create_event_busses();
        let price_alert_service = PriceAlertService::new(receivers);
    }
}
