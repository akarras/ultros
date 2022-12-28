use std::{collections::HashMap, sync::Arc};
use tokio::sync::{
    mpsc::{self, Receiver},
    RwLock,
};
use ultros_db::{entity::active_listing, world_cache::AnySelector};

use crate::event::EventReceivers;

pub(crate) struct PriceUndercutData {
    pub(crate) item_id: i32,
    pub(crate) undercut_by: i32,
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
                self.check_listings(&l).await;
            }
        }
    }

    async fn check_listings(&self, listings: &[active_listing::Model]) {
        // events *should* be one item at a time so this reduce is safe. if that ever changes, need to fix this.
        listings.iter().map(|i| (i.price_per_unit, i.item_id)).min();
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
    use crate::event::create_event_busses;

    use super::PriceAlertService;

    #[tokio::test]
    async fn test_price_alert() {
        let (senders, receivers) = create_event_busses();
        let price_alert_service = PriceAlertService::new(receivers);
    }
}
