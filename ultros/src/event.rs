use std::sync::Arc;

use tokio::sync::broadcast::channel;
use ultros_api_types::{
    user::OwnedRetainer,
    websocket::{ListingEventData, SaleEventData},
};
use ultros_db::entity::*;

pub(crate) type EventBus<T> = tokio::sync::broadcast::Receiver<EventType<Arc<T>>>;
pub(crate) type EventProducer<T> = tokio::sync::broadcast::Sender<EventType<Arc<T>>>;

#[derive(Clone, Debug)]
pub enum EventType<T> {
    Remove(T),
    Add(T),
    Update(T),
}

impl<T> AsRef<T> for EventType<T> {
    fn as_ref(&self) -> &T {
        match self {
            EventType::Remove(t) => t,
            EventType::Add(t) => t,
            EventType::Update(t) => t,
        }
    }
}

impl<T> EventType<Arc<T>> {
    pub(crate) fn removed(data: T) -> Self {
        EventType::Remove(Arc::new(data))
    }

    pub(crate) fn added(data: T) -> Self {
        EventType::Add(Arc::new(data))
    }

    pub(crate) fn updated(data: T) -> Self {
        EventType::Update(Arc::new(data))
    }
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

#[derive(Clone)]
pub(crate) struct EventSenders {
    pub(crate) retainers: EventProducer<OwnedRetainer>,
    pub(crate) listings: EventProducer<ListingEventData>,
    pub(crate) alerts: EventProducer<alert::Model>,
    pub(crate) retainer_undercut: EventProducer<alert_retainer_undercut::Model>,
    pub(crate) history: EventProducer<SaleEventData>,
}

/// Base event type for communicating across different parts of the app
#[derive(Debug)]
pub(crate) struct EventReceivers {
    pub(crate) retainers: EventBus<OwnedRetainer>,
    pub(crate) listings: EventBus<ListingEventData>,
    pub(crate) alerts: EventBus<alert::Model>,
    pub(crate) retainer_undercut: EventBus<alert_retainer_undercut::Model>,
    pub(crate) history: EventBus<SaleEventData>,
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
