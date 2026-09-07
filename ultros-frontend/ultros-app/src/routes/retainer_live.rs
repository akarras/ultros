//! Live-update wiring for `/retainers/listings` and `/retainers/undercuts`.
//!
//! Listing websocket events are deltas (an `Added`/`Removed`/`Updated` carries
//! only the listings that changed), so a page cannot recompute "cheapest on
//! this world" from an event alone. Both pages therefore refetch their
//! resource when a relevant event lands, debounced so a burst on a busy
//! world becomes one request.

use ultros_api_types::websocket::{EventType, FilterPredicate, ServerClient};
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
