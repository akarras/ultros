use std::sync::Arc;

use tokio::sync::broadcast::channel;
use ultros_db::entity::*;
use universalis::{ItemId, WorldId};

pub(crate) type EventBus<T> = tokio::sync::broadcast::Receiver<EventType<Arc<T>>>;
pub(crate) type EventProducer<T> = tokio::sync::broadcast::Sender<EventType<Arc<T>>>;

#[derive(Clone, Debug)]
pub enum EventType<T> {
    Remove(T),
    Add(T),
    Update(T),
}

pub(crate) fn create_event_busses() -> (EventSenders, EventReceivers) {
    let (retainer_sender, retainer_receiver) = channel(10);
    let (listing_sender, listing_receiver) = channel(100);
    let (alert_sender, alert_receiver) = channel(10);
    let (retainer_undercut_sender, retainer_undercut_receiver) = channel(40);
    let (history_sender, history_receiver) = channel(40);
    (
        EventSenders {
            retainers: retainer_sender,
            listings: listing_sender,
            alerts: alert_sender,
            retainer_undercut: retainer_undercut_sender,
            history: history_sender,
        },
        EventReceivers {
            retainers: retainer_receiver,
            listings: listing_receiver,
            alerts: alert_receiver,
            retainer_undercut: retainer_undercut_receiver,
            history: history_receiver,
        },
    )
}

#[derive(Debug)]
pub(crate) struct ListingData {
    pub(crate) item_id: ItemId,
    pub(crate) world_id: WorldId,
    pub(crate) listings: Vec<active_listing::Model>,
}

#[derive(Clone)]
pub(crate) struct EventSenders {
    pub(crate) retainers: EventProducer<retainer::Model>,
    pub(crate) listings: EventProducer<ListingData>,
    pub(crate) alerts: EventProducer<alert::Model>,
    pub(crate) retainer_undercut: EventProducer<alert_retainer_undercut::Model>,
    pub(crate) history: EventProducer<Vec<sale_history::Model>>,
}

/// Base event type for communicating across different parts of the app
#[derive(Debug)]
pub(crate) struct EventReceivers {
    pub(crate) retainers: EventBus<retainer::Model>,
    pub(crate) listings: EventBus<ListingData>,
    pub(crate) alerts: EventBus<alert::Model>,
    pub(crate) retainer_undercut: EventBus<alert_retainer_undercut::Model>,
    pub(crate) history: EventBus<Vec<sale_history::Model>>,
}

impl Clone for EventReceivers {
    fn clone(&self) -> Self {
        Self {
            retainers: self.retainers.resubscribe(),
            listings: self.listings.resubscribe(),
            alerts: self.alerts.resubscribe(),
            retainer_undercut: self.retainer_undercut.resubscribe(),
            history: self.history.resubscribe(),
        }
    }
}
