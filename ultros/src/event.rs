use std::sync::Arc;

use tokio::sync::broadcast::{channel, error::RecvError};
use tracing::warn;
use ultros_api_types::{
    user::OwnedRetainer,
    websocket::{ListEventData, ListingEventData, SaleEventData},
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

    #[allow(unused)]
    pub(crate) fn updated(data: T) -> Self {
        EventType::Update(Arc::new(data))
    }
}

/// Ring size for the listings bus.
///
/// A tokio broadcast channel drops the *oldest* value once the ring is full, so
/// anything a slow consumer hasn't drained yet is lost — see
/// `ultros_analyzer_bus_lagged_total` for when that happens.
///
/// Sizing comes from the catch-up service, which bursts far harder than the
/// websocket does: `UpdateService::check_items` walks item ids in chunks of 100
/// and drives each chunk through `buffer_unordered(50)`, and every item emits
/// *two* listing events (Add + Remove). So a single chunk can put ~200 events on
/// the bus as fast as Postgres finishes writing, while the analyzer's listings
/// loop drains them one at a time — and its `remove_listing` path does a DB
/// round-trip per removed listing. The old value of 100 was smaller than one
/// chunk's burst, i.e. guaranteed to lag during any full sweep.
///
/// 1024 gives ~5 chunks of headroom. Slots hold `Arc<ListingEventData>`, and a
/// typical event carries only a handful of listings (~1-2 KiB), so the ring
/// costs single-digit MiB even when completely full.
const LISTINGS_BUS_SIZE: usize = 1024;

/// Ring size for the sale-history bus.
///
/// Same burst source as [`LISTINGS_BUS_SIZE`] — one sale event per item, so ~100
/// per catch-up chunk against 40 slots previously. Lag here is worse than on the
/// listings bus because the analyzer's history loop is the *only* thing feeding
/// the ClickHouse dual-write: dropped events vanish from both the in-RAM sale
/// history and ClickHouse while Postgres stays correct, so the two silently
/// drift apart.
///
/// 512 is ~5 chunks of headroom. `SaleEventData` is a plain vec of sales with no
/// retainer payload, so slots are cheaper than the listings bus'.
const HISTORY_BUS_SIZE: usize = 512;

pub(crate) fn create_event_busses() -> (EventSenders, EventReceivers) {
    let (retainer_sender, retainer_receiver) = channel(10);
    let (listing_sender, listing_receiver) = channel(LISTINGS_BUS_SIZE);
    let (alert_sender, alert_receiver) = channel(10);
    let (retainer_undercut_sender, retainer_undercut_receiver) = channel(40);
    let (history_sender, history_receiver) = channel(HISTORY_BUS_SIZE);
    let (list_sender, list_receiver) = channel(40);
    (
        EventSenders {
            retainers: retainer_sender,
            listings: listing_sender,
            alerts: alert_sender,
            retainer_undercut: retainer_undercut_sender,
            history: history_sender,
            lists: list_sender,
        },
        EventReceivers {
            retainers: retainer_receiver,
            listings: listing_receiver,
            alerts: alert_receiver,
            retainer_undercut: retainer_undercut_receiver,
            history: history_receiver,
            lists: list_receiver,
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
    pub(crate) lists: EventProducer<ListEventData>,
}

/// Base event type for communicating across different parts of the app
#[derive(Debug)]
pub(crate) struct EventReceivers {
    pub(crate) retainers: EventBus<OwnedRetainer>,
    pub(crate) listings: EventBus<ListingEventData>,
    pub(crate) alerts: EventBus<alert::Model>,
    pub(crate) retainer_undercut: EventBus<alert_retainer_undercut::Model>,
    pub(crate) history: EventBus<SaleEventData>,
    pub(crate) lists: EventBus<ListEventData>,
}

/// Unwraps a broadcast `recv()` result, reporting lag instead of swallowing it.
///
/// `if let Ok(..) = rx.recv().await` looks harmless but hides the one error that
/// actually costs data: [`RecvError::Lagged`] means the channel already threw
/// away `n` values because this receiver couldn't keep up. Postgres still has
/// them (every producer writes there first), so nothing 500s — the in-RAM caches
/// and the ClickHouse mirror just quietly drift away from the truth.
///
/// Returns `Some` when a value arrived and `None` on lag or close. Callers
/// should keep looping on `None`: after a lag the receiver is still live and
/// positioned at the oldest surviving value.
pub(crate) fn handle_bus_recv<T>(bus: &'static str, result: Result<T, RecvError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(RecvError::Lagged(skipped)) => {
            metrics::counter!("ultros_analyzer_bus_lagged_total", "bus" => bus).increment(skipped);
            warn!(
                bus,
                skipped,
                "consumer fell behind the event bus; dropped events never reach the \
                 in-RAM caches or ClickHouse (Postgres is unaffected)"
            );
            None
        }
        Err(RecvError::Closed) => {
            warn!(bus, "event bus closed, no further events will arrive");
            None
        }
    }
}

impl Clone for EventReceivers {
    fn clone(&self) -> Self {
        Self {
            retainers: self.retainers.resubscribe(),
            listings: self.listings.resubscribe(),
            alerts: self.alerts.resubscribe(),
            retainer_undercut: self.retainer_undercut.resubscribe(),
            history: self.history.resubscribe(),
            lists: self.lists.resubscribe(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delivered_value_passes_through() {
        assert_eq!(handle_bus_recv::<i32>("test", Ok(7)), Some(7));
    }

    #[test]
    fn lag_and_close_yield_nothing() {
        assert_eq!(
            handle_bus_recv::<i32>("test", Err(RecvError::Lagged(12))),
            None
        );
        assert_eq!(handle_bus_recv::<i32>("test", Err(RecvError::Closed)), None);
    }

    /// The bug this replaces: `if let Ok(..) = recv()` treats a lag exactly like
    /// a delivered value it happened not to match, so a burst larger than the
    /// ring vanishes with no log, no metric, and no way to tell from the
    /// outside. Here the lag is surfaced once and the loop keeps consuming
    /// every value that survived in the ring.
    #[tokio::test]
    async fn a_burst_larger_than_the_ring_reports_lag_then_resumes() {
        let (tx, mut rx) = channel(2);
        for i in 0..5 {
            tx.send(i).unwrap();
        }

        // The oldest three were dropped by the channel; the next recv says so.
        assert_eq!(handle_bus_recv("test", rx.recv().await), None);

        // Everything still in the ring is delivered normally afterwards.
        assert_eq!(handle_bus_recv("test", rx.recv().await), Some(3));
        assert_eq!(handle_bus_recv("test", rx.recv().await), Some(4));

        // And the receiver keeps working for values sent after the lag.
        tx.send(5).unwrap();
        assert_eq!(handle_bus_recv("test", rx.recv().await), Some(5));
    }
}
