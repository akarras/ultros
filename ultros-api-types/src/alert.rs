use serde::{Deserialize, Serialize};

use crate::world_helper::AnySelector;

/// What kind of condition the alert checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AlertTrigger {
    /// Fire when any listing for this item drops to or below `price_threshold`.
    BelowThreshold {
        item_id: i32,
        world_selector: AnySelector,
        price_threshold: i32,
        hq_only: bool,
    },
}

/// Where to send a fired alert.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum AlertDelivery {
    /// Send a Discord DM to the user. The user_id is derived from the auth session, not the request body.
    DiscordDm,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAlertRequest {
    pub trigger: AlertTrigger,
    pub delivery: AlertDelivery,
    /// Defaults to 3600 (1 hour) if omitted.
    pub cooldown_seconds: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateAlertRequest {
    pub enabled: Option<bool>,
    pub price_threshold: Option<i32>,
    pub cooldown_seconds: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alert {
    pub id: i32,
    pub trigger: AlertTrigger,
    pub delivery: AlertDelivery,
    pub enabled: bool,
    pub cooldown_seconds: i32,
    pub last_fired_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertEvent {
    pub id: i64,
    pub alert_id: i32,
    pub fired_at: chrono::DateTime<chrono::Utc>,
    pub item_id: i32,
    pub matched_listing_id: Option<i64>,
    pub matched_price: Option<i32>,
    pub delivered: bool,
    pub delivery_error: Option<String>,
}
