//! Compact Postgres `LISTEN/NOTIFY` wake-up channel definitions and emitters.
//!
//! `NOTIFY` is strictly a best-effort wake-up signal — never a durable queue.
//! Every payload carries only compact identifiers plus the emitting process's
//! `origin` token (so a replica ignores its own echoes) and stays well under
//! Postgres's 8,000-byte notify limit. Authoritative state always lives in
//! Postgres and is fetched by the receiving replica on wake-up or reconnect.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::UltrosDb;

/// Auth revocation wake-ups (`AuthUserCache` early eviction across replicas).
pub const CHANNEL_AUTH_REVOKE: &str = "ultros_auth_revoke";
/// List document / list metadata / list activity wake-ups.
pub const CHANNEL_LIST_DOC: &str = "ultros_list_doc";
/// Notification inbox & plugin notification wake-ups.
pub const CHANNEL_NOTIFICATION: &str = "ultros_notification";
/// Alert CRUD wake-ups (`AlertManager` + trackers).
pub const CHANNEL_ALERT: &str = "ultros_alert";
/// Owned retainer add/remove/sync wake-ups (`AlertManager` undercut + sale trackers).
pub const CHANNEL_RETAINER: &str = "ultros_retainer";

/// Every channel subscribed by the cross-replica `PgListener` bridge.
pub const ALL_WAKEUP_CHANNELS: [&str; 5] = [
    CHANNEL_AUTH_REVOKE,
    CHANNEL_LIST_DOC,
    CHANNEL_NOTIFICATION,
    CHANNEL_ALERT,
    CHANNEL_RETAINER,
];

/// Defensive upper bound well below Postgres's 8,000-byte `NOTIFY` payload cap.
pub const MAX_WAKEUP_PAYLOAD_BYTES: usize = 4096;

/// Maximum `list_activity` row IDs included in a single list wake-up payload.
pub const MAX_LIST_WAKEUP_ACTIVITY_IDS: usize = 16;

/// Maximum `alert_retainer_undercut` IDs included in an alert wake-up payload.
pub const MAX_ALERT_WAKEUP_UNDERCUT_IDS: usize = 32;

/// Generate a random 16-hex-char process identifier used to ignore self-echoes
/// on Postgres `LISTEN/NOTIFY` channels.
pub(crate) fn generate_notify_origin() -> String {
    let mut bytes = [0u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        let fallback = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
            ^ (std::process::id() as u64).rotate_left(32);
        bytes = fallback.to_le_bytes();
    }
    let mut out = String::with_capacity(16);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthRevokeWakeup {
    /// Emitting process origin ID.
    pub o: String,
    /// Optional 64-char hex SHA-256 hash of a revoked bearer token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
    /// Optional Discord user ID whose account was deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub u: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListDocWakeup {
    /// Emitting process origin ID.
    pub o: String,
    /// List ID that changed.
    pub l: i32,
    /// Optional newly recorded `list_activity.id` rows (bounded).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub a: Vec<i64>,
    /// True when the list itself was deleted.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub d: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationWakeupKind {
    #[serde(rename = "a")]
    Add,
    #[serde(rename = "u")]
    Update,
    #[serde(rename = "w")]
    PluginWake,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationWakeup {
    /// Emitting process origin ID.
    pub o: String,
    /// Owner Discord user ID.
    pub u: i64,
    /// Optional `alert_event.id` for `Add` / `Update`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub e: Option<i64>,
    /// Event kind.
    pub k: NotificationWakeupKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertWakeupKind {
    #[serde(rename = "a")]
    Add,
    #[serde(rename = "u")]
    Update,
    #[serde(rename = "r")]
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertWakeup {
    /// Emitting process origin ID.
    pub o: String,
    /// Alert ID.
    pub a: i32,
    /// Change kind.
    pub k: AlertWakeupKind,
    /// Related `alert_retainer_undercut.id` rows (needed especially for `Remove`
    /// after cascade delete).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ru: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetainerWakeup {
    /// Emitting process origin ID.
    pub o: String,
    /// Owner Discord user ID whose owned retainers changed.
    pub u: i64,
}

/// Emit `SELECT pg_notify($1, $2)` on any Postgres `ConnectionTrait` (pool or
/// active transaction). Inside a transaction Postgres delivers the notification
/// atomically on commit and discards it on rollback.
pub(crate) async fn pg_notify_on<C: ConnectionTrait>(
    conn: &C,
    channel: &str,
    payload: &str,
) -> Result<(), sea_orm::DbErr> {
    if conn.get_database_backend() != DbBackend::Postgres {
        return Ok(());
    }
    if payload.len() > MAX_WAKEUP_PAYLOAD_BYTES {
        warn!(
            channel,
            bytes = payload.len(),
            "refusing to emit oversized pg_notify wakeup payload"
        );
        return Ok(());
    }
    conn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT pg_notify($1, $2)",
        [channel.into(), payload.into()],
    ))
    .await?;
    Ok(())
}

impl UltrosDb {
    /// Best-effort `NOTIFY` emission on Postgres. Never fails the caller because
    /// the authoritative DB write has already committed and reconnect/timer
    /// reconciliation covers missed wake-ups.
    pub async fn notify_wakeup(&self, channel: &str, payload: &str) {
        if let Err(error) = pg_notify_on(&self.db, channel, payload).await {
            warn!(?error, channel, "failed to emit pg_notify wakeup");
        }
    }

    /// Wake remote replicas that a list document, list metadata, or list
    /// activity changed. Sends IDs only — never CRDT bytes.
    pub async fn notify_list_doc_changed(&self, list_id: i32, activity_ids: &[i64]) {
        let payload = ListDocWakeup {
            o: self.notify_origin().to_string(),
            l: list_id,
            a: activity_ids
                .iter()
                .copied()
                .take(MAX_LIST_WAKEUP_ACTIVITY_IDS)
                .collect(),
            d: false,
        };
        if let Ok(json) = serde_json::to_string(&payload) {
            self.notify_wakeup(CHANNEL_LIST_DOC, &json).await;
        }
    }

    /// Wake remote replicas that a list was deleted so collaborators purge
    /// cached document state and drop subscriptions even though the DB row is gone.
    pub async fn notify_list_deleted(&self, list_id: i32) {
        let payload = ListDocWakeup {
            o: self.notify_origin().to_string(),
            l: list_id,
            a: Vec::new(),
            d: true,
        };
        if let Ok(json) = serde_json::to_string(&payload) {
            self.notify_wakeup(CHANNEL_LIST_DOC, &json).await;
        }
    }

    /// Wake remote replicas that an alert inbox event was added or updated.
    pub async fn notify_notification_event(
        &self,
        owner: i64,
        event_id: i64,
        kind: NotificationWakeupKind,
    ) {
        let payload = NotificationWakeup {
            o: self.notify_origin().to_string(),
            u: owner,
            e: Some(event_id),
            k: kind,
        };
        if let Ok(json) = serde_json::to_string(&payload) {
            self.notify_wakeup(CHANNEL_NOTIFICATION, &json).await;
        }
    }

    /// Wake remote replicas' plugin SSE streams for `owner`.
    pub async fn notify_plugin_notification_wake(&self, owner: i64) {
        let payload = NotificationWakeup {
            o: self.notify_origin().to_string(),
            u: owner,
            e: None,
            k: NotificationWakeupKind::PluginWake,
        };
        if let Ok(json) = serde_json::to_string(&payload) {
            self.notify_wakeup(CHANNEL_NOTIFICATION, &json).await;
        }
    }

    /// Wake remote `AlertManager` replicas that an alert changed.
    pub async fn notify_alert_changed(
        &self,
        alert_id: i32,
        kind: AlertWakeupKind,
        retainer_undercut_ids: &[i32],
    ) {
        let payload = AlertWakeup {
            o: self.notify_origin().to_string(),
            a: alert_id,
            k: kind,
            ru: retainer_undercut_ids
                .iter()
                .copied()
                .take(MAX_ALERT_WAKEUP_UNDERCUT_IDS)
                .collect(),
        };
        if let Ok(json) = serde_json::to_string(&payload) {
            self.notify_wakeup(CHANNEL_ALERT, &json).await;
        }
    }

    /// Wake remote `AlertManager` replicas that `owner`'s owned retainers changed.
    pub async fn notify_retainer_changed(&self, owner: i64) {
        let payload = RetainerWakeup {
            o: self.notify_origin().to_string(),
            u: owner,
        };
        if let Ok(json) = serde_json::to_string(&payload) {
            self.notify_wakeup(CHANNEL_RETAINER, &json).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wakeup_payloads_round_trip_and_stay_compact() {
        let auth = AuthRevokeWakeup {
            o: "0123456789abcdef".into(),
            t: Some("a".repeat(64)),
            u: Some(42),
        };
        let auth_json = serde_json::to_string(&auth).unwrap();
        assert!(auth_json.len() < 200, "auth payload too big: {auth_json}");
        assert_eq!(
            serde_json::from_str::<AuthRevokeWakeup>(&auth_json).unwrap(),
            auth
        );

        let list = ListDocWakeup {
            o: "0123456789abcdef".into(),
            l: 12345,
            a: (1..=MAX_LIST_WAKEUP_ACTIVITY_IDS as i64).collect(),
            d: false,
        };
        let list_json = serde_json::to_string(&list).unwrap();
        assert!(list_json.len() < 256, "list payload too big: {list_json}");
        assert_eq!(
            serde_json::from_str::<ListDocWakeup>(&list_json).unwrap(),
            list
        );

        let notif = NotificationWakeup {
            o: "0123456789abcdef".into(),
            u: 9876543210,
            e: Some(555),
            k: NotificationWakeupKind::Add,
        };
        let notif_json = serde_json::to_string(&notif).unwrap();
        assert!(notif_json.len() < 128);
        assert_eq!(
            serde_json::from_str::<NotificationWakeup>(&notif_json).unwrap(),
            notif
        );

        let alert = AlertWakeup {
            o: "0123456789abcdef".into(),
            a: 77,
            k: AlertWakeupKind::Remove,
            ru: vec![1, 2, 3],
        };
        let alert_json = serde_json::to_string(&alert).unwrap();
        assert!(alert_json.len() < 128);
        assert_eq!(
            serde_json::from_str::<AlertWakeup>(&alert_json).unwrap(),
            alert
        );

        let retainer = RetainerWakeup {
            o: "0123456789abcdef".into(),
            u: 9876543210,
        };
        let retainer_json = serde_json::to_string(&retainer).unwrap();
        assert!(retainer_json.len() < 96);
        assert_eq!(
            serde_json::from_str::<RetainerWakeup>(&retainer_json).unwrap(),
            retainer
        );
    }

    #[test]
    fn notify_origin_is_16_lowercase_hex_chars() {
        let origin = generate_notify_origin();
        assert_eq!(origin.len(), 16);
        assert!(
            origin
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        );
    }
}
