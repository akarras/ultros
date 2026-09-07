//! Live-update wiring for `/retainers/listings` and `/retainers/undercuts`.
//!
//! Listing websocket events are deltas (an `Added`/`Removed`/`Updated` carries
//! only the listings that changed), so a page cannot recompute "cheapest on
//! this world" from an event alone. Both pages therefore refetch their
//! resource when a relevant event lands, debounced so a burst on a busy
//! world becomes one request.

use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use chrono::{DateTime, Utc};
use leptos::prelude::*;
use ultros_api_types::websocket::{EventType, FilterPredicate, ServerClient, SocketMessageType};
use ultros_api_types::world_helper::AnySelector;

/// A `(world_id, item_id)` pair one of the user's retainers currently lists.
pub(crate) type ListedPair = (i32, i32);

/// `Items(<unique item ids>) AND (World(w1) OR World(w2) OR ...)` over the
/// unique worlds in `pairs`. `None` when there is nothing to subscribe to.
pub(crate) fn retainer_market_filter(pairs: &[ListedPair]) -> Option<FilterPredicate> {
    let mut item_ids: Vec<i32> = pairs.iter().map(|(_, item)| *item).collect();
    item_ids.sort_unstable();
    item_ids.dedup();
    let mut world_ids: Vec<i32> = pairs.iter().map(|(world, _)| *world).collect();
    world_ids.sort_unstable();
    world_ids.dedup();

    let mut worlds = world_ids.into_iter();
    let first = FilterPredicate::World(AnySelector::World(worlds.next()?));
    let worlds = worlds.fold(first, |acc, world| {
        acc.or(FilterPredicate::World(AnySelector::World(world)))
    });
    Some(FilterPredicate::Items(item_ids).and(worlds))
}

/// True when `message` should trigger a refetch for a page showing `pairs`:
/// a `Listings` event whose `(world_id, item_id)` is listed, or `Stale`
/// while anything is listed. Server-side filtering already narrows events;
/// this is the client-side guard, the same role
/// `is_list_market_update_relevant` plays for lists.
pub(crate) fn is_retainer_update_relevant(message: &ServerClient, pairs: &[ListedPair]) -> bool {
    match message {
        ServerClient::Listings(event) => {
            let data = match event {
                EventType::Added(data) | EventType::Removed(data) | EventType::Updated(data) => {
                    data
                }
            };
            pairs.contains(&(data.world_id, data.item_id))
        }
        ServerClient::Stale { .. } => !pairs.is_empty(),
        _ => false,
    }
}

/// Trailing debounce for refetches. On wasm this is a `gloo_timers::Timeout`;
/// under `ssr` the whole hook is inert (the realtime client never fires), so
/// the timer is a no-op there.
#[derive(Default)]
struct RefetchDebounce {
    #[cfg(not(feature = "ssr"))]
    pending: Option<gloo_timers::callback::Timeout>,
}

#[cfg(not(feature = "ssr"))]
const DEBOUNCE_MS: u32 = 1_500;

impl RefetchDebounce {
    /// Replace any pending timer with a fresh one that runs `refetch` after
    /// [`DEBOUNCE_MS`]. Dropping the previous `Timeout` cancels it.
    fn schedule(&mut self, refetch: impl FnOnce() + 'static) {
        #[cfg(not(feature = "ssr"))]
        {
            self.pending = Some(gloo_timers::callback::Timeout::new(DEBOUNCE_MS, refetch));
        }
        #[cfg(feature = "ssr")]
        {
            let _ = refetch;
        }
    }

    /// Drop any pending timer without running it.
    fn cancel(&mut self) {
        #[cfg(not(feature = "ssr"))]
        {
            self.pending = None;
        }
    }
}

/// Signals a page hands to `RealtimeStatus`.
#[derive(Clone, Copy)]
pub(crate) struct RetainerLive {
    pub status: Signal<String>,
    pub last_update: Signal<Option<DateTime<Utc>>>,
}

/// Subscribe a retainer page to listing events for everything its retainers
/// currently list, and call `refetch` (debounced) when one lands.
///
/// `pairs` is derived from the page's resource: `None` while it is loading or
/// errored, `Some(vec)` once it resolves. The subscription is rebuilt every
/// time `pairs` resolves, so a newly listed item that shows up after a
/// refetch is covered without a page reload (mirrors the list page).
pub(crate) fn use_retainer_live(
    pairs: Signal<Option<Vec<ListedPair>>>,
    refetch: impl Fn() + Clone + 'static,
) -> RetainerLive {
    let (status, set_status) = signal("connecting".to_string());
    let (last_update, set_last_update) = signal(None::<DateTime<Utc>>);
    let subscription = StoredValue::new_local(None::<RealtimeSubscription>);
    let debounce = StoredValue::new_local(RefetchDebounce::default());
    let realtime = use_realtime();

    Effect::new(move |_| {
        let Some(realtime) = realtime.clone() else {
            set_status.set("offline".to_string());
            return;
        };
        // `None` covers loading, errored, and (if the resource clears its
        // value mid-refetch) the window between a refetch and its answer —
        // keep the existing subscription alive across all of those.
        let Some(pairs) = pairs.get() else {
            return;
        };
        subscription.update_value(|sub| *sub = None);
        debounce.update_value(RefetchDebounce::cancel);
        let Some(filter) = retainer_market_filter(&pairs) else {
            // Nothing listed, nothing to watch: a perpetual "connecting"
            // pulse would be a lie, so show the socket as idle instead.
            set_status.set("offline".to_string());
            return;
        };
        set_status.set("connecting".to_string());
        let refetch = refetch.clone();
        let sub = realtime.subscribe_market(filter, SocketMessageType::Listings, move |message| {
            match message {
                ServerClient::Subscribed { .. } => {
                    set_status.set("live".to_string());
                }
                ServerClient::Listings(_) => {
                    if !is_retainer_update_relevant(&message, &pairs) {
                        return;
                    }
                    set_status.set("live".to_string());
                    set_last_update.set(Some(Utc::now()));
                    let refetch = refetch.clone();
                    debounce.update_value(|d| d.schedule(move || refetch()));
                }
                ServerClient::Stale { .. } | ServerClient::Error { .. } => {
                    set_status.set("reconnecting".to_string());
                    set_last_update.set(Some(Utc::now()));
                    debounce.update_value(RefetchDebounce::cancel);
                    refetch();
                }
                _ => {}
            }
        });
        subscription.set_value(Some(sub));
    });

    on_cleanup(move || {
        subscription.update_value(|sub| *sub = None);
        debounce.update_value(RefetchDebounce::cancel);
    });

    RetainerLive {
        status: status.into(),
        last_update: last_update.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::websocket::{EventType, ListingEventData, SaleEventData};

    fn listings_event(world_id: i32, item_id: i32) -> ServerClient {
        ServerClient::Listings(EventType::Added(ListingEventData {
            item_id,
            world_id,
            listings: Vec::new(),
        }))
    }

    /// Collects every `World(AnySelector::World(id))` leaf under an `Or` tree.
    fn world_ids(pred: &FilterPredicate) -> Vec<i32> {
        match pred {
            FilterPredicate::World(AnySelector::World(id)) => vec![*id],
            FilterPredicate::Or((a, b)) => {
                let mut ids = world_ids(a);
                ids.extend(world_ids(b));
                ids
            }
            other => panic!("unexpected predicate in world tree: {other:?}"),
        }
    }

    #[test]
    fn empty_pairs_yield_no_filter() {
        assert!(retainer_market_filter(&[]).is_none());
    }

    #[test]
    fn single_pair_is_items_and_world() {
        let filter = retainer_market_filter(&[(34, 5)]).expect("filter");
        let FilterPredicate::And((items, worlds)) = filter else {
            panic!("expected And, got {filter:?}");
        };
        let FilterPredicate::Items(ids) = *items else {
            panic!("expected Items, got {items:?}");
        };
        assert_eq!(ids, vec![5]);
        assert_eq!(world_ids(&worlds), vec![34]);
    }

    #[test]
    fn many_pairs_dedupe_items_and_or_worlds() {
        let filter = retainer_market_filter(&[(34, 5), (34, 7), (40, 5), (40, 9)]).expect("filter");
        let FilterPredicate::And((items, worlds)) = filter else {
            panic!("expected And, got {filter:?}");
        };
        let FilterPredicate::Items(mut ids) = *items else {
            panic!("expected Items, got {items:?}");
        };
        ids.sort_unstable();
        assert_eq!(ids, vec![5, 7, 9]);
        let mut worlds = world_ids(&worlds);
        worlds.sort_unstable();
        assert_eq!(worlds, vec![34, 40]);
    }

    #[test]
    fn matching_listing_event_is_relevant() {
        assert!(is_retainer_update_relevant(
            &listings_event(34, 5),
            &[(34, 5)]
        ));
    }

    #[test]
    fn listing_event_for_other_world_or_item_is_not_relevant() {
        let pairs = [(34, 5)];
        assert!(!is_retainer_update_relevant(&listings_event(40, 5), &pairs));
        assert!(!is_retainer_update_relevant(&listings_event(34, 6), &pairs));
    }

    #[test]
    fn stale_is_relevant_only_with_pairs() {
        let stale = ServerClient::Stale { subscription_id: 1 };
        assert!(is_retainer_update_relevant(&stale, &[(34, 5)]));
        assert!(!is_retainer_update_relevant(&stale, &[]));
    }

    #[test]
    fn sales_are_never_relevant() {
        let sales = ServerClient::Sales(EventType::Added(SaleEventData { sales: Vec::new() }));
        assert!(!is_retainer_update_relevant(&sales, &[(34, 5)]));
    }
}
