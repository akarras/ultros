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
    /// Fire when any item in the referenced list drops to or below that item's
    /// per-row `target_price`. The list-scoped trigger lets a single alert
    /// follow every item in a shopping list (including items added later).
    ListItemThreshold { list_id: i32 },
}

/// Where to send a fired alert.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum AlertDelivery {
    /// Send a Discord DM to the user. The user_id is derived from the auth session, not the request body.
    DiscordDm,
    /// POST a Discord-shaped embed to a user-provided webhook URL (typically a Discord channel webhook).
    Webhook { url: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAlertRequest {
    pub trigger: AlertTrigger,
    /// Deprecated for new clients; if `endpoint_ids` is empty an endpoint is created from this.
    /// Kept for backward compatibility with existing client-side code until the drawer is migrated.
    #[serde(default)]
    pub delivery: Option<AlertDelivery>,
    /// Endpoints to attach to this alert. Required if `delivery` is None.
    #[serde(default)]
    pub endpoint_ids: Vec<i32>,
    /// Defaults to 3600 (1 hour) if omitted.
    pub cooldown_seconds: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateAlertRequest {
    pub enabled: Option<bool>,
    pub price_threshold: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alert {
    pub id: i32,
    pub trigger: AlertTrigger,
    /// Deprecated; reflects the first endpoint's method for old clients. New clients should use `endpoint_ids`.
    pub delivery: AlertDelivery,
    pub endpoint_ids: Vec<i32>,
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

/// Delivery channel for a notification endpoint. Mirrors the `method` discriminator
/// stored in the `notification_endpoint.method` DB column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "PascalCase")]
pub enum EndpointMethod {
    DiscordDm {
        user_id: i64,
    },
    DiscordChannel {
        channel_id: i64,
        /// Resolved channel name (e.g. "general"). Populated by the server when the
        /// endpoint is created via the live serenity context. `None` for legacy rows
        /// that were created before name resolution was added — clients should fall
        /// back to displaying the channel id in that case.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel_name: Option<String>,
        /// Discord guild that owns this channel. Populated alongside `channel_name`.
        /// Used by the frontend to show the guild context and by the server to scope
        /// the admin check on update operations.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guild_id: Option<i64>,
        /// Resolved guild name (e.g. "My Free Company"). Populated alongside
        /// `channel_name`. Display-only — never trusted for permission checks.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guild_name: Option<String>,
    },
    Webhook {
        url: String,
    },
    /// Browser-side push subscription, owned by a row in `push_subscription`.
    /// Created via `POST /api/v1/push/subscribe`, not the generic endpoints CRUD.
    WebPush {
        subscription_id: i32,
    },
}

/// Body for `POST /api/v1/push/subscribe`. The browser obtains `endpoint`, `p256dh`,
/// and `auth` from the result of `PushManager.subscribe(...)`; `user_agent` is
/// `navigator.userAgent` (best-effort, used only for the endpoint label).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatePushSubscriptionRequest {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
    pub user_agent: Option<String>,
}

/// Response shape for `GET /api/v1/push/vapid-public-key`. Single field so the
/// frontend doesn't have to special-case a bare string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VapidPublicKey {
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: i32,
    pub name: String,
    #[serde(flatten)]
    pub method: EndpointMethod,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateEndpointRequest {
    pub name: String,
    #[serde(flatten)]
    pub method: EndpointMethod,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateEndpointRequest {
    pub name: Option<String>,
    pub method: Option<EndpointMethod>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResendResult {
    pub delivered: bool,
    pub error: Option<String>,
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoint_method_serializes_with_method_tag() {
        let m = EndpointMethod::DiscordDm { user_id: 42 };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, json!({"method": "DiscordDm", "user_id": 42}));
    }

    #[test]
    fn endpoint_serializes_with_flattened_method() {
        let e = Endpoint {
            id: 7,
            name: "Test".to_string(),
            method: EndpointMethod::Webhook {
                url: "https://example.invalid".into(),
            },
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v,
            json!({"id": 7, "name": "Test", "method": "Webhook", "url": "https://example.invalid"})
        );
    }

    #[test]
    fn create_endpoint_request_round_trips() {
        let req = CreateEndpointRequest {
            name: "My channel".into(),
            method: EndpointMethod::DiscordChannel {
                channel_id: 9,
                channel_name: None,
                guild_id: None,
                guild_name: None,
            },
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: CreateEndpointRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn update_endpoint_request_all_none_round_trips() {
        let req = UpdateEndpointRequest {
            name: None,
            method: None,
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: UpdateEndpointRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn update_endpoint_request_name_only_round_trips() {
        let req = UpdateEndpointRequest {
            name: Some("renamed".to_string()),
            method: None,
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: UpdateEndpointRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn update_endpoint_request_method_change_round_trips() {
        let req = UpdateEndpointRequest {
            name: None,
            method: Some(EndpointMethod::Webhook {
                url: "https://discord.com/api/webhooks/1/abc".into(),
            }),
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: UpdateEndpointRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
    }
}
