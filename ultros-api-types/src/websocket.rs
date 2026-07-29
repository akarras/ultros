use serde::{Deserialize, Serialize};

use crate::{
    ActiveListing, SaleHistory, UnknownCharacter,
    retainer::Retainer,
    world_helper::{AnySelector, WorldHelper},
};

pub trait PredicateDataSources {
    fn world(&self) -> AnySelector;
    fn item(&self) -> i32;
    fn price(&self) -> i32;
    fn character(&self) -> Option<&str>;
    fn retainer(&self) -> Option<&str>;
}

impl PredicateDataSources for (SaleHistory, UnknownCharacter) {
    fn world(&self) -> AnySelector {
        AnySelector::World(self.0.world_id)
    }

    fn item(&self) -> i32 {
        self.0.sold_item_id
    }

    fn price(&self) -> i32 {
        self.0.price_per_item
    }

    fn character(&self) -> Option<&str> {
        Some(&self.1.name)
    }

    fn retainer(&self) -> Option<&str> {
        None
    }
}

impl PredicateDataSources for (ActiveListing, Retainer) {
    fn world(&self) -> AnySelector {
        AnySelector::World(self.0.world_id)
    }

    fn item(&self) -> i32 {
        self.0.item_id
    }

    fn price(&self) -> i32 {
        self.0.price_per_unit
    }

    fn character(&self) -> Option<&str> {
        None
    }

    fn retainer(&self) -> Option<&str> {
        Some(self.1.name.as_str())
    }
}

impl FilterPredicate {
    pub fn filter<T: PredicateDataSources>(&self, world_helper: &WorldHelper, data: &T) -> bool {
        match self {
            FilterPredicate::World(w) => world_helper
                .lookup_selector(data.world())
                .and_then(|filter_world| {
                    world_helper
                        .lookup_selector(*w)
                        .map(|event_world| filter_world.is_in(&event_world))
                })
                .unwrap_or(true),
            FilterPredicate::Item(i) => data.item() == *i,
            FilterPredicate::Items(items) => items.contains(&data.item()),
            FilterPredicate::Retainer(r) => data
                .retainer()
                .map(|re| re.eq_ignore_ascii_case(r))
                .unwrap_or(true), // default to true
            FilterPredicate::Character(character) => data
                .character()
                .map(|c| c.eq_ignore_ascii_case(character))
                .unwrap_or(true),
            FilterPredicate::And((a, b)) => {
                a.filter(world_helper, data) && b.filter(world_helper, data)
            }
            FilterPredicate::Or((a, b)) => {
                a.filter(world_helper, data) || b.filter(world_helper, data)
            }
            FilterPredicate::PriceAtLeast(price) => data.price() >= *price,
            FilterPredicate::PriceAtMost(price) => data.price() <= *price,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum FilterPredicate {
    World(AnySelector),
    Item(i32),
    Items(Vec<i32>),
    /// Is technically only a valid filter against a listing
    Retainer(String),
    /// Is technically only valid against a sale history
    Character(String),
    PriceAtLeast(i32),
    PriceAtMost(i32),
    /// Combines two filter predicates with an AND
    And((Box<FilterPredicate>, Box<FilterPredicate>)),
    /// Combines two filter predicates with an OR
    Or((Box<FilterPredicate>, Box<FilterPredicate>)),
}

impl FilterPredicate {
    pub fn and(self, other: FilterPredicate) -> FilterPredicate {
        Self::And((Box::new(self), Box::new(other)))
    }

    pub fn or(self, other: FilterPredicate) -> FilterPredicate {
        Self::Or((Box::new(self), Box::new(other)))
    }
}

/// Decides whether a websocket listing/sale event is relevant to an analyzer view.
/// An event is relevant if:
/// 1. It matches the item being viewed.
/// 2. It matches either the target sell world OR it matches the buy filter (if one is active).
pub fn is_analyzer_event_relevant(
    event_item_id: i32,
    event_world_id: i32,
    viewed_item_id: i32,
    sell_world_id: i32,
    buy_filter: Option<AnySelector>,
    world_helper: &WorldHelper,
) -> bool {
    if event_item_id != viewed_item_id {
        return false;
    }

    // Always relevant if it happened on our target sell world.
    if event_world_id == sell_world_id {
        return true;
    }

    // If there is a buy filter (world or datacenter), it's relevant if it matches that filter.
    if let Some(filter) = buy_filter {
        let event_any = AnySelector::World(event_world_id);
        if let (Some(event_res), Some(filter_res)) = (
            world_helper.lookup_selector(event_any),
            world_helper.lookup_selector(filter),
        ) {
            return event_res.is_in(&filter_res);
        }
    }

    false
}

/// Decides whether a websocket market update should refresh the analyzer.
///
/// Listing updates are relevant only when they match one of the currently
/// analyzed item ids and the analyzer's sell/buy world state. `Stale` messages
/// are scoped to the subscription that produced them, so any non-empty analyzer
/// subscription should refetch when one arrives.
pub fn is_analyzer_market_update_relevant(
    message: &ServerClient,
    viewed_item_ids: &[i32],
    sell_world_id: i32,
    buy_filter: Option<AnySelector>,
    world_helper: &WorldHelper,
) -> bool {
    if viewed_item_ids.is_empty() || sell_world_id == 0 {
        return false;
    }

    match message {
        ServerClient::Listings(event) => {
            let data = match event {
                EventType::Added(data) | EventType::Removed(data) | EventType::Updated(data) => {
                    data
                }
            };
            viewed_item_ids.iter().any(|viewed_item_id| {
                is_analyzer_event_relevant(
                    data.item_id,
                    data.world_id,
                    *viewed_item_id,
                    sell_world_id,
                    buy_filter,
                    world_helper,
                )
            })
        }
        ServerClient::Stale { .. } => true,
        _ => false,
    }
}

/// Decides whether a websocket market update should refresh a list view.
///
/// Listing updates carry an item id and are relevant only when that item is
/// present in the current list view. `Stale` messages are already routed to a
/// specific subscription and do not carry an item id, so they are relevant as
/// long as the current list view has subscribed market items.
pub fn is_list_market_update_relevant(message: &ServerClient, list_item_ids: &[i32]) -> bool {
    match message {
        ServerClient::Listings(event) => {
            let data = match event {
                EventType::Added(data) | EventType::Removed(data) | EventType::Updated(data) => {
                    data
                }
            };
            list_item_ids.contains(&data.item_id)
        }
        ServerClient::Stale { .. } => !list_item_ids.is_empty(),
        _ => false,
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum EventType<T> {
    Added(T),
    Removed(T),
    Updated(T),
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ListingEventData {
    pub item_id: i32,
    pub world_id: i32,
    pub listings: Vec<(ActiveListing, Retainer)>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct SaleEventData {
    pub sales: Vec<(SaleHistory, UnknownCharacter)>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum ListEventData {
    List(crate::list::List),
    ListItem(crate::list::ListItem),
    Activity(crate::list::ListActivity),
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum ServerClient {
    Sales(EventType<SaleEventData>),
    Listings(EventType<ListingEventData>),
    ListUpdate(EventType<ListEventData>),
    SubscriptionEvent {
        subscription_id: u64,
        event: Box<ServerClient>,
    },
    Subscribed {
        subscription_id: u64,
    },
    Unsubscribed {
        subscription_id: u64,
    },
    Stale {
        subscription_id: u64,
    },
    Error {
        message: String,
    },
    SubscriptionCreated,
    SocketConnected,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum SocketMessageType {
    Listings,
    Sales,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum ClientMessage {
    AddSubscribe {
        #[serde(default)]
        subscription_id: Option<u64>,
        filter: FilterPredicate,
        msg_type: SocketMessageType,
    },
    Unsubscribe {
        subscription_id: u64,
    },
    SubscribeList {
        #[serde(default)]
        subscription_id: Option<u64>,
        list_id: i32,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AlertsRx {
    Undercuts { margin: i32 },
    WatchCharacter { name: String },
    Ping(Vec<u8>),
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, PartialOrd, Ord)]
pub struct UndercutRetainer {
    pub id: i32,
    pub name: String,
    pub undercut_amount: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AlertsTx {
    RetainerUndercut {
        item_id: i32,
        item_name: String,
        /// List of all the retainers that were just undercut
        undercut_retainers: Vec<UndercutRetainer>,
    },
    ItemPurchased {
        item_id: i32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Datacenter, Region, World, WorldData};
    use chrono::NaiveDateTime;

    fn helper() -> WorldHelper {
        WorldData {
            regions: vec![Region {
                id: 1,
                name: "NA".into(),
                datacenters: vec![Datacenter {
                    id: 10,
                    name: "Aether".into(),
                    region_id: 1,
                    worlds: vec![
                        World {
                            id: 100,
                            name: "Adamantoise".into(),
                            datacenter_id: 10,
                        },
                        World {
                            id: 101,
                            name: "Cactuar".into(),
                            datacenter_id: 10,
                        },
                    ],
                }],
            }],
        }
        .into()
    }

    fn listing(world_id: i32, item_id: i32, price: i32) -> (ActiveListing, Retainer) {
        (
            ActiveListing {
                id: 1,
                world_id,
                item_id,
                retainer_id: 7,
                price_per_unit: price,
                quantity: 1,
                hq: false,
                timestamp: NaiveDateTime::default(),
            },
            Retainer {
                id: 7,
                world_id,
                name: "Bob".into(),
                retainer_city_id: 1,
            },
        )
    }

    #[test]
    fn predicate_world_matches_same_world() {
        let h = helper();
        let data = listing(100, 5, 1000);
        let pred = FilterPredicate::World(AnySelector::World(100));
        assert!(pred.filter(&h, &data));
    }

    #[test]
    fn predicate_world_matches_when_event_world_is_inside_filter_dc() {
        let h = helper();
        let data = listing(100, 5, 1000);
        let pred = FilterPredicate::World(AnySelector::Datacenter(10));
        assert!(pred.filter(&h, &data));
    }

    #[test]
    fn predicate_world_misses_for_unknown_event_world() {
        // Per impl, unknown lookup defaults to `true` (no filtering applied).
        let h = helper();
        let data = listing(999, 5, 1000);
        let pred = FilterPredicate::World(AnySelector::World(100));
        assert!(pred.filter(&h, &data));
    }

    #[test]
    fn predicate_item_id_matches_exact() {
        let h = helper();
        let data = listing(100, 42, 100);
        assert!(FilterPredicate::Item(42).filter(&h, &data));
        assert!(!FilterPredicate::Item(43).filter(&h, &data));
    }

    #[test]
    fn predicate_retainer_matches_exact_name() {
        let h = helper();
        let data = listing(100, 1, 1);
        assert!(FilterPredicate::Retainer("Bob".into()).filter(&h, &data));
        assert!(!FilterPredicate::Retainer("Alice".into()).filter(&h, &data));
    }

    #[test]
    fn predicate_character_defaults_to_true_when_listing_has_no_character() {
        let h = helper();
        let data = listing(100, 1, 1);
        // ActiveListing+Retainer.character() returns None, so filter returns true.
        assert!(FilterPredicate::Character("anyone".into()).filter(&h, &data));
    }

    #[test]
    fn predicate_price_at_least_passes_when_price_is_at_or_above_threshold() {
        let h = helper();
        let data = listing(100, 1, 100);
        // PriceAtLeast(threshold) means data.price() >= threshold
        assert!(FilterPredicate::PriceAtLeast(100).filter(&h, &data));
        assert!(FilterPredicate::PriceAtLeast(50).filter(&h, &data));
        assert!(!FilterPredicate::PriceAtLeast(200).filter(&h, &data));
    }

    #[test]
    fn predicate_price_at_most_passes_when_price_is_at_or_below_threshold() {
        let h = helper();
        let data = listing(100, 1, 100);
        // PriceAtMost(threshold) means data.price() <= threshold
        assert!(FilterPredicate::PriceAtMost(100).filter(&h, &data));
        assert!(FilterPredicate::PriceAtMost(200).filter(&h, &data));
        assert!(!FilterPredicate::PriceAtMost(50).filter(&h, &data));
    }

    #[test]
    fn predicate_and_or_combinators_short_circuit_correctly() {
        let h = helper();
        let data = listing(100, 42, 100);
        let item_match = FilterPredicate::Item(42);
        let item_miss = FilterPredicate::Item(99);
        // and: both true ⇒ true
        let both = item_match
            .clone()
            .and(FilterPredicate::Retainer("Bob".into()));
        assert!(both.filter(&h, &data));
        // and: one false ⇒ false
        let one = item_miss
            .clone()
            .and(FilterPredicate::Retainer("Bob".into()));
        assert!(!one.filter(&h, &data));
        // or: one true ⇒ true
        let or_one = item_miss.clone().or(item_match.clone());
        assert!(or_one.filter(&h, &data));
        // or: none true ⇒ false
        let or_none = item_miss.or(FilterPredicate::Retainer("Nobody".into()));
        assert!(!or_none.filter(&h, &data));
    }

    #[test]
    fn predicate_serde_roundtrip_through_json() {
        let pred = FilterPredicate::Item(42)
            .and(FilterPredicate::PriceAtMost(100))
            .or(FilterPredicate::Retainer("Bob".into()));
        let s = serde_json::to_string(&pred).unwrap();
        let back: FilterPredicate = serde_json::from_str(&s).unwrap();
        // Round-trip the round-trip to compare structurally via JSON.
        let s2 = serde_json::to_string(&back).unwrap();
        assert_eq!(s, s2);
    }

    #[test]
    fn predicate_data_sources_for_listing_returns_expected_fields() {
        let h = helper();
        let data = listing(100, 7, 1234);
        let _ = h; // unused, just to mirror call sites
        assert_eq!(data.world(), AnySelector::World(100));
        assert_eq!(data.item(), 7);
        assert_eq!(data.price(), 1234);
        assert!(data.character().is_none());
        assert_eq!(data.retainer(), Some("Bob"));
    }

    #[test]
    fn undercut_retainer_orders_by_struct_field_order() {
        let mut v = [
            UndercutRetainer {
                id: 2,
                name: "B".into(),
                undercut_amount: 0,
            },
            UndercutRetainer {
                id: 1,
                name: "A".into(),
                undercut_amount: 0,
            },
        ];
        v.sort();
        assert_eq!(v[0].id, 1);
        assert_eq!(v[1].id, 2);
    }

    #[test]
    fn analyzer_matching_logic() {
        let h = helper();
        let sell_world = 100;
        let item_id = 42;

        // 1. Matches item and sell world
        assert!(is_analyzer_event_relevant(
            item_id, 100, item_id, sell_world, None, &h
        ));

        // 2. Unrelated item id (ignored)
        assert!(!is_analyzer_event_relevant(
            99, 100, item_id, sell_world, None, &h
        ));

        // 3. Unrelated world, no filter (ignored)
        assert!(!is_analyzer_event_relevant(
            item_id, 101, item_id, sell_world, None, &h
        ));

        // 4. Unrelated world, matching filter (world level)
        assert!(is_analyzer_event_relevant(
            item_id,
            101,
            item_id,
            sell_world,
            Some(AnySelector::World(101)),
            &h
        ));

        // 5. Unrelated world, matching filter (datacenter level)
        assert!(is_analyzer_event_relevant(
            item_id,
            101,
            item_id,
            sell_world,
            Some(AnySelector::Datacenter(10)),
            &h
        ));

        // 6. Matching item id on unrelated world when filtered (ignored)
        // (Event on world 999, but filter is world 101)
        assert!(!is_analyzer_event_relevant(
            item_id,
            999,
            item_id,
            sell_world,
            Some(AnySelector::World(101)),
            &h
        ));

        // 7. Matching item id on unrelated world when filtered by another world (ignored)
        assert!(!is_analyzer_event_relevant(
            item_id,
            101,
            item_id,
            sell_world,
            Some(AnySelector::World(100)),
            &h
        ));
    }

    #[test]
    fn analyzer_matching_logic_empty_states() {
        let h = helper();

        // Verify "no item viewed" state handling (should return false)
        assert!(!is_analyzer_event_relevant(42, 100, 0, 100, None, &h));
        assert!(!is_analyzer_event_relevant(42, 100, 42, 0, None, &h));
    }

    fn analyzer_market_message(item_id: i32, world_id: i32) -> ServerClient {
        ServerClient::Listings(EventType::Updated(ListingEventData {
            item_id,
            world_id,
            listings: vec![],
        }))
    }

    #[test]
    fn analyzer_market_update_matches_current_item_on_sell_world() {
        let h = helper();
        let message = analyzer_market_message(42, 100);

        assert!(is_analyzer_market_update_relevant(
            &message,
            &[42],
            100,
            None,
            &h
        ));
    }

    #[test]
    fn analyzer_market_update_ignores_unrelated_item_id() {
        let h = helper();
        let message = analyzer_market_message(42, 100);

        assert!(!is_analyzer_market_update_relevant(
            &message,
            &[7],
            100,
            None,
            &h
        ));
    }

    #[test]
    fn analyzer_market_update_matches_buy_filter_world_or_datacenter() {
        let h = helper();
        let message = analyzer_market_message(42, 101);

        assert!(is_analyzer_market_update_relevant(
            &message,
            &[42],
            100,
            Some(AnySelector::World(101)),
            &h
        ));
        assert!(is_analyzer_market_update_relevant(
            &message,
            &[42],
            100,
            Some(AnySelector::Datacenter(10)),
            &h
        ));
    }

    #[test]
    fn analyzer_market_update_ignores_unmatched_world_without_buy_filter() {
        let h = helper();
        let message = analyzer_market_message(42, 101);

        assert!(!is_analyzer_market_update_relevant(
            &message,
            &[42],
            100,
            None,
            &h
        ));
    }

    #[test]
    fn analyzer_market_stale_event_matches_non_empty_subscription_only() {
        let h = helper();
        let message = ServerClient::Stale { subscription_id: 1 };

        assert!(is_analyzer_market_update_relevant(
            &message,
            &[42],
            100,
            None,
            &h
        ));
        assert!(!is_analyzer_market_update_relevant(
            &message,
            &[],
            100,
            None,
            &h
        ));
    }

    fn list_market_message(item_id: i32) -> ServerClient {
        ServerClient::Listings(EventType::Updated(ListingEventData {
            item_id,
            world_id: 100,
            listings: vec![],
        }))
    }

    #[test]
    fn list_market_update_matches_item_id_in_current_list() {
        let message = list_market_message(42);

        assert!(is_list_market_update_relevant(&message, &[42]));
    }

    #[test]
    fn list_market_update_ignores_unrelated_item_id() {
        let message = list_market_message(42);

        assert!(!is_list_market_update_relevant(&message, &[7]));
    }

    #[test]
    fn list_market_update_ignores_empty_list() {
        let message = list_market_message(42);

        assert!(!is_list_market_update_relevant(&message, &[]));
        assert!(!is_list_market_update_relevant(
            &ServerClient::Stale { subscription_id: 1 },
            &[]
        ));
    }

    #[test]
    fn list_market_update_matches_mixed_item_list() {
        let message = list_market_message(42);

        assert!(is_list_market_update_relevant(&message, &[7, 42, 99]));
        assert!(!is_list_market_update_relevant(&message, &[7, 99]));
    }

    #[test]
    fn list_market_stale_event_is_relevant_to_non_empty_subscription() {
        let message = ServerClient::Stale { subscription_id: 1 };

        assert!(is_list_market_update_relevant(&message, &[42]));
    }
}
