use crate::{ItemId, ListingView, WorldId};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize, Serializer};
use serde_with::{formats::Flexible, serde_as, TimestampSeconds};
use std::fmt::{Display, Formatter};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum SubscribeMode {
    Subscribe,
    Unsubscribe,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "event")]
pub enum WSMessage {
    #[serde(rename = "listings/add")]
    ListingsAdd {
        item: ItemId,
        world: WorldId,
        listings: Vec<ListingView>,
    },
    #[serde(rename = "listings/remove")]
    ListingsRemove {
        item: ItemId,
        world: WorldId,
        listings: Vec<ListingView>,
    },
    #[serde(rename = "sales/add")]
    SalesAdd {
        item: ItemId,
        world: WorldId,
        sales: Vec<SaleView>,
    },
    #[serde(rename = "sales/remove")]
    SalesRemove {
        item: ItemId,
        world: WorldId,
        sales: Vec<SaleView>,
    },
}

impl From<&WSMessage> for EventChannel {
    fn from(ws: &WSMessage) -> Self {
        match ws {
            WSMessage::ListingsAdd { .. } => EventChannel::ListingsAdd,
            WSMessage::ListingsRemove { .. } => EventChannel::ListingsRemove,
            WSMessage::SalesAdd { .. } => EventChannel::SalesAdd,
            WSMessage::SalesRemove { .. } => EventChannel::SalesRemove,
        }
    }
}

impl From<&WSMessage> for ItemId {
    fn from(ws: &WSMessage) -> Self {
        match ws {
            WSMessage::ListingsAdd { item, .. } => *item,
            WSMessage::ListingsRemove { item, .. } => *item,
            WSMessage::SalesAdd { item, .. } => *item,
            WSMessage::SalesRemove { item, .. } => *item,
        }
    }
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SaleView {
    pub hq: bool,
    pub price_per_unit: i32,
    pub quantity: i32,
    #[serde_as(as = "TimestampSeconds<i64, Flexible>")]
    pub timestamp: DateTime<Local>,
    pub on_mannequin: bool,
    pub world_name: Option<String>,
    pub world_id: Option<WorldId>,
    pub buyer_name: String,
    pub total: i32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EventResponse {
    pub event: EventChannel,
    pub item: i32,
    pub world: WorldId,
    pub listings: Vec<ListingView>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum EventChannel {
    #[serde(rename = "listings/add")]
    ListingsAdd,
    #[serde(rename = "listings/remove")]
    ListingsRemove,
    #[serde(rename = "sales/add")]
    SalesAdd,
    #[serde(rename = "sales/remove")]
    SalesRemove,
}

impl Display for EventChannel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            EventChannel::ListingsAdd => write!(f, "listings/add"),
            EventChannel::ListingsRemove => write!(f, "listings/remove"),
            EventChannel::SalesAdd => write!(f, "sales/add"),
            EventChannel::SalesRemove => write!(f, "sales/remove"),
        }
    }
}

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Hash, Clone, Copy)]
pub(crate) struct WorldFilter(WorldId);

impl WorldFilter {
    pub(crate) fn new(id: WorldId) -> Self {
        Self(id)
    }
}

impl Display for WorldFilter {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{world={}}}", self.0 .0)
    }
}

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Hash, Clone)]
pub(crate) struct Channel(EventChannel, Option<WorldFilter>);

impl Channel {
    pub(crate) fn new(event_channel: EventChannel, world_filter: Option<WorldFilter>) -> Self {
        Self(event_channel, world_filter)
    }
}

impl Serialize for Channel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = format!(
            "{}{}",
            self.0,
            self.1.as_ref().map(|m| m.to_string()).unwrap_or_default()
        );
        serializer.serialize_str(&value)
    }
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebSocketSubscriptionUpdate {
    pub(crate) event: SubscribeMode,
    pub(crate) channel: Channel,
}

impl WebSocketSubscriptionUpdate {
    pub(crate) fn new(event: SubscribeMode, channel: Channel) -> Self {
        Self { event, channel }
    }
}
