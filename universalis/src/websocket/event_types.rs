use crate::{ListingView, WorldId};
use serde::{Deserialize, Serialize, Serializer};
use std::fmt::{Display, Formatter};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum SubscribeMode {
    Subscribe,
    Unsubscribe,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EventResponse {
    event: String,
    item: i32,
    world: WorldId,
    listings: Vec<ListingView>,
}

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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
    event: SubscribeMode,
    channel: Channel,
}

impl WebSocketSubscriptionUpdate {
    pub(crate) fn new(event: SubscribeMode, channel: Channel) -> Self {
        Self { event, channel }
    }
}
