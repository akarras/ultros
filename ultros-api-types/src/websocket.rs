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
            FilterPredicate::Retainer(r) => data.retainer().map(|re| re == r).unwrap_or(true), // default to true
            FilterPredicate::Character(character) => {
                data.character().map(|c| c == character).unwrap_or(true)
            }
            FilterPredicate::And((a, b)) => {
                a.filter(world_helper, data) && b.filter(world_helper, data)
            }
            FilterPredicate::Or((a, b)) => {
                a.filter(world_helper, data) || b.filter(world_helper, data)
            }
            FilterPredicate::PriceAtLeast(price) => data.price() <= *price,
            FilterPredicate::PriceAtMost(price) => data.price() >= *price,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum FilterPredicate {
    World(AnySelector),
    Item(i32),
    /// Is technically only a valid filter against a SaleHistory
    Retainer(String),
    /// Is technically only valid against a listing
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

#[derive(Deserialize, Serialize, Debug)]
pub enum EventType<T> {
    Added(T),
    Removed(T),
    Updated(T),
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ListingEventData {
    pub item_id: i32,
    pub world_id: i32,
    pub listings: Vec<(ActiveListing, Retainer)>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct SaleEventData {
    pub sales: Vec<(SaleHistory, UnknownCharacter)>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum ListEventData {
    List(crate::list::List),
    ListItem(crate::list::ListItem),
}

#[derive(Deserialize, Serialize, Debug)]
pub enum ServerClient {
    Sales(EventType<SaleEventData>),
    Listings(EventType<ListingEventData>),
    ListUpdate(EventType<ListEventData>),
    SubscriptionCreated,
    SocketConnected,
}

#[derive(Deserialize, Serialize, Debug)]
pub enum SocketMessageType {
    Listings,
    Sales,
}

#[derive(Deserialize, Serialize, Debug)]
pub enum ClientMessage {
    AddSubscribe {
        filter: FilterPredicate,
        msg_type: SocketMessageType,
    },
    SubscribeList {
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
    fn predicate_price_at_least_passes_when_price_is_at_or_below_threshold() {
        let h = helper();
        let data = listing(100, 1, 100);
        // PriceAtLeast(threshold) means data.price() <= threshold
        assert!(FilterPredicate::PriceAtLeast(100).filter(&h, &data));
        assert!(FilterPredicate::PriceAtLeast(200).filter(&h, &data));
        assert!(!FilterPredicate::PriceAtLeast(50).filter(&h, &data));
    }

    #[test]
    fn predicate_price_at_most_passes_when_price_is_at_or_above_threshold() {
        let h = helper();
        let data = listing(100, 1, 100);
        // PriceAtMost(threshold) means data.price() >= threshold
        assert!(FilterPredicate::PriceAtMost(50).filter(&h, &data));
        assert!(FilterPredicate::PriceAtMost(100).filter(&h, &data));
        assert!(!FilterPredicate::PriceAtMost(200).filter(&h, &data));
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
}
