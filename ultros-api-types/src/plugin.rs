//! Versioned device-client contracts. Credentials deliberately do not implement Debug.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const SCOPES: &[(&str, &str)] = &[
    ("notifications:read", "Receive game notifications"),
    ("notifications:ack", "Acknowledge game notifications"),
    (
        "notifications:mark-read",
        "Mark delivered notifications read",
    ),
    ("lists:read", "Read shopping lists"),
    ("lists:write", "Edit shopping list rows"),
    ("retainers:read", "Read private characters and retainers"),
    ("retainers:register", "Register characters and retainers"),
    ("retainers:upload", "Upload private retainer listings"),
];

pub fn valid_scopes(scopes: &[String]) -> bool {
    !scopes.is_empty()
        && scopes.len() <= SCOPES.len()
        && scopes.iter().all(|s| SCOPES.iter().any(|(id, _)| s == id))
        && scopes
            .iter()
            .enumerate()
            .all(|(i, s)| !scopes[..i].contains(s))
        && [
            ("lists:write", "lists:read"),
            ("retainers:register", "retainers:read"),
            ("retainers:upload", "retainers:read"),
            ("notifications:read", "notifications:ack"),
            ("notifications:ack", "notifications:mark-read"),
            ("notifications:mark-read", "notifications:read"),
        ]
        .iter()
        .all(|(scope, dependency)| {
            !scopes.iter().any(|s| s == scope) || scopes.iter().any(|s| s == dependency)
        })
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePairing {
    pub label: String,
    pub scopes: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct PairingChallenge {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// A dedicated short-lived selection credential, never the device code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_uri_complete: Option<String>,
    pub expires_in: u32,
    pub interval: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollPairing {
    pub device_code: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshSession {
    pub refresh_token: String,
}

#[derive(Serialize, Deserialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: u32,
    pub device_id: String,
    pub account_id: String,
    pub account_name: String,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceSession {
    pub id: String,
    pub label: String,
    pub account_id: String,
    pub account_name: String,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub endpoint_id: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginWebsiteInfo {
    pub enabled: bool,
    pub localhost: bool,
    pub origin: String,
    pub download_url: Option<String>,
    #[serde(default)]
    pub repository_url: Option<String>,
    #[serde(default)]
    pub latest_version: Option<String>,
    #[serde(default)]
    pub minimum_version: Option<String>,
}

// Browser consent data stays in the browser's memory, never in SSR resources,
// URLs, local storage or logs. These deliberately do not implement Debug.
#[derive(Clone, Serialize, Deserialize)]
pub struct BrowserApproval {
    pub account_name: String,
    pub csrf: String,
    pub has_pending_link: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BrowserSelection {
    pub account_name: String,
    pub code: String,
    pub label: String,
    pub scopes: Vec<String>,
    pub csrf: String,
    pub nonce: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BrowserDevices {
    pub account_name: String,
    pub csrf: String,
    pub sessions: Vec<DeviceSession>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BrowserDecision {
    pub approved: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedRetainer {
    pub content_id: String,
    pub name: String,
    pub city: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObserveCharacter {
    pub content_id: String,
    pub name: String,
    pub home_world: i32,
    pub retainers: Vec<ObservedRetainer>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterRetainers {
    pub operation_id: String,
    pub character: ObserveCharacter,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrivateRetainer {
    pub id: String,
    pub name: String,
    pub city: Option<i32>,
    pub registered: bool,
    pub revision: i64,
    pub observed_at: DateTime<Utc>,
    pub grouping_conflict: bool,
    /// A unique existing public market record, never manufactured from game data.
    #[serde(default)]
    pub public_retainer_id: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnverifiedCharacter {
    pub id: String,
    pub name: String,
    pub home_world: i32,
    pub registered: bool,
    pub verified: bool,
    pub upload_enabled: bool,
    pub capture_epoch: i64,
    pub observed_at: DateTime<Utc>,
    pub retainers: Vec<PrivateRetainer>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadConsent {
    pub enabled: bool,
    #[serde(default)]
    pub delete_snapshots: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PrivateListing {
    pub slot: u8,
    pub item_id: i32,
    pub hq: bool,
    pub quantity: i32,
    pub unit_price: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListingCapture {
    pub capture_id: String,
    pub base_revision: i64,
    pub capture_epoch: i64,
    pub observed_at: DateTime<Utc>,
    pub complete: bool,
    pub listings: Vec<PrivateListing>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaptureReceipt {
    pub revision: i64,
    pub hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetainerSnapshot {
    pub retainer_id: String,
    pub revision: i64,
    pub received_at: DateTime<Utc>,
    pub observed_at: DateTime<Utc>,
    pub listings: Vec<PrivateListing>,
}

/// Private, owner-scoped price comparison; never a public market upload.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetainerPriceLookup {
    pub retainer_id: String,
    pub name: String,
    pub world_id: i32,
    pub world_name: String,
    pub source: String,
    pub observed_at: Option<DateTime<Utc>>,
    pub fetched_at: DateTime<Utc>,
    pub listings: Vec<RetainerPriceQuote>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetainerPriceQuote {
    pub slot: u8,
    pub item_id: i32,
    pub hq: bool,
    pub quantity: i32,
    pub current_price: i64,
    pub competitor_price: Option<i64>,
    pub market_checked_at: Option<DateTime<Utc>>,
    pub suggested_price: Option<i64>,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotificationAction {
    OpenItem { item_id: i32 },
    OpenList { list_id: i32 },
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameDelivery {
    pub id: String,
    pub event_id: Option<i64>,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub action: NotificationAction,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeliveryBatch {
    pub deliveries: Vec<GameDelivery>,
    pub poll_after_ms: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryIds {
    pub ids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationRequest {
    pub operation_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncList {
    pub version: String,
    pub update: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SyncedList {
    pub version: String,
    pub kind: String,
    pub payload: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grants_are_allowlisted_and_dependencies_are_explicit() {
        assert!(valid_scopes(&["lists:read".into(), "lists:write".into()]));
        for invalid in [
            vec![],
            vec!["admin"],
            vec!["lists:write"],
            vec!["notifications:read"],
            vec!["lists:read", "lists:read"],
        ] {
            assert!(!valid_scopes(
                &invalid.into_iter().map(str::to_string).collect::<Vec<_>>()
            ));
        }
        assert!(valid_scopes(
            &SCOPES
                .iter()
                .map(|(id, _)| id.to_string())
                .collect::<Vec<_>>()
        ));
    }
}
