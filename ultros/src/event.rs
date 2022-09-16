use std::sync::Arc;

use tokio::sync::broadcast::channel;
use ultros_db::entity::*;

pub(crate) type EventBus<T> = tokio::sync::broadcast::Receiver<EventType<Arc<T>>>;
pub(crate) type EventProducer<T> = tokio::sync::broadcast::Sender<EventType<Arc<T>>>;

#[derive(Clone, Debug)]
pub(crate) enum EventType<T> {
    Remove(T),
    Add(T),
    Update(T),
}

pub(crate) fn create_event_busses() -> (EventSenders, EventReceivers) {
    let (retainer_sender, retainer_receiver) = channel(10);
    let (listing_sender, listing_receiver) = channel(100);
    let (alert_sender, alert_receiver) = channel(10);
    let (retainer_undercut_sender, retainer_undercut_receiver) = channel(40);
    (
        EventSenders {
            retainers: retainer_sender,
            listings: listing_sender,
            alerts: alert_sender,
            retainer_undercut: retainer_undercut_sender,
        },
        EventReceivers {
            retainers: retainer_receiver,
            listings: listing_receiver,
            alerts: alert_receiver,
            retainer_undercut: retainer_undercut_receiver,
        },
    )
}

#[derive(Debug, Clone)]
pub(crate) struct EventSenders {
    pub(crate) retainers: EventProducer<retainer::Model>,
    pub(crate) listings: EventProducer<Vec<active_listing::Model>>,
    pub(crate) alerts: EventProducer<alert::Model>,
    pub(crate) retainer_undercut: EventProducer<alert_retainer_undercut::Model>,
}

/// Base event type for communicating across different parts of the app
#[derive(Debug)]
pub(crate) struct EventReceivers {
    pub(crate) retainers: EventBus<retainer::Model>,
    pub(crate) listings: EventBus<Vec<active_listing::Model>>,
    pub(crate) alerts: EventBus<alert::Model>,
    pub(crate) retainer_undercut: EventBus<alert_retainer_undercut::Model>,
}

impl Clone for EventReceivers {
    fn clone(&self) -> Self {
        Self {
            retainers: self.retainers.resubscribe(),
            listings: self.listings.resubscribe(),
            alerts: self.alerts.resubscribe(),
            retainer_undercut: self.retainer_undercut.resubscribe(),
        }
    }
}
