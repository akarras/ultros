use serde::{Deserialize, Serialize};

use crate::{
    world_helper::{AnySelector, WorldHelper},
    ActiveListing, Retainer, SaleHistory, UnknownCharacter,
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

#[derive(Deserialize, Serialize, Debug)]
pub enum ServerClient {
    Sales(EventType<SaleEventData>),
    Listings(EventType<ListingEventData>),
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
}
