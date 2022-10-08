use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use ultros_db::entity::active_listing;

use crate::{event::EventReceivers, world_cache::AnySelector};

#[derive(Debug)]
pub(crate) struct PriceAlertDetails {
    /// If price is below this threshold, then send the alert
    pub(crate) price_threshold: i32,
    pub(crate) item_id: i32,
    /// If the price is within this selector, then send the alert
    pub(crate) travel_amount: AnySelector,
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
        let e = event_receiver.clone();
        tokio::spawn(async move { i.start_listener(e).await });
        instance
    }

    async fn start_listener(&self, mut event_receiver: EventReceivers) {
        loop {
            if let Ok(listing) = event_receiver.listings.recv().await {
                match listing {
                    crate::event::EventType::Add(l) => {}
                    crate::event::EventType::Update(l) => {}
                    _ => {}
                }
            }
        }
    }

    async fn check_listings(&self, listings: &[active_listing::Model]) {
        // events *should* be one item at a time so this reduce is safe. if that ever changes, need to fix this.
        listings.iter().map(|i| (i.price_per_unit, i.item_id)).min();
    }
}
