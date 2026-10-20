//! Notification-inbox fire helper.
//!
//! Every alert tracker calls [`record_fire`] to durably accept the fire before
//! the outbox sends externally. REST reuses [`alert_event_to_api`]
//! so the websocket push and the paginated history endpoint always agree on
//! the wire shape.

use crate::delivery::PushOptions;
use tracing::error;
use ultros_api_types::alert::AlertEvent;
use ultros_db::{NewAlertEvent, UltrosDb, entity::alert_event};

use crate::event::{EventProducer, EventType, NotificationEvent};

/// The single DB-model -> wire-type mapping for alert events.
pub fn alert_event_to_api(m: alert_event::Model) -> AlertEvent {
    AlertEvent {
        id: m.id,
        alert_id: m.alert_id,
        fired_at: m.fired_at.with_timezone(&chrono::Utc),
        item_id: m.item_id,
        matched_listing_id: m.matched_listing_id,
        matched_price: m.matched_price,
        delivered: m.delivered,
        accepted_at: m.accepted_at.map(|dt| dt.with_timezone(&chrono::Utc)),
        pending_destinations: m.pending_destinations,
        delivery_error: m.delivery_error,
        read_at: m.read_at.map(|dt| dt.with_timezone(&chrono::Utc)),
        title: m.title,
        body: m.body,
        click_url: m.click_url,
    }
}

/// Parameters for [`record_fire`]: everything [`NewAlertEvent`] needs, plus
/// the `owner` used to address the notification at the right inbox (the DB
/// row itself has no owner column — it's reached through `alert_id`).
pub struct AlertFire<'a> {
    pub alert_id: i32,
    pub owner: i64,
    pub item_id: i32,
    pub matched_listing_id: Option<i64>,
    pub matched_price: Option<i32>,
    pub title: &'a str,
    pub body: &'a str,
    pub click_url: &'a str,
    pub push: &'a PushOptions,
}

/// Record an alert fire and broadcast it to the owner's open inbox sockets.
///
/// Returns true only after the event, cooldown and every destination's work
/// commit together. Callers advance memory cooldowns only after true.
pub async fn record_fire(
    db: &UltrosDb,
    notifications: &EventProducer<NotificationEvent>,
    fire: AlertFire<'_>,
) -> bool {
    let AlertFire {
        alert_id,
        owner,
        item_id,
        matched_listing_id,
        matched_price,
        title,
        body,
        click_url,
        push,
    } = fire;
    let inserted = db
        .accept_alert_fire(owner,NewAlertEvent {
            alert_id,
            item_id,
            matched_listing_id,
            matched_price,
            delivered:false,
            delivery_error:None,
            title: title.to_string(),
            body: body.to_string(),
            click_url: click_url.to_string(),
        },serde_json::json!({"click_url":push.click_url,"ttl_secs":push.ttl_secs,"topic":push.topic}))
        .await;
    let model = match inserted {
        Ok(Some(model)) => model,
        Ok(None) => return false,
        Err(e) => {
            error!("failed to record alert_event for alert {alert_id}: {e}");
            return false;
        }
    };
    let event = alert_event_to_api(model);
    let notification = NotificationEvent { owner, event };
    tracing::debug!(
        owner = notification.owner,
        alert_id = notification.event.alert_id,
        "queued alert fire for the notification-inbox bus"
    );
    let _ = notifications.send(EventType::added(notification));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn model() -> alert_event::Model {
        alert_event::Model {
            id: 7,
            alert_id: 3,
            fired_at: chrono::FixedOffset::east_opt(9 * 3600)
                .unwrap()
                .with_ymd_and_hms(2026, 9, 15, 12, 0, 0)
                .unwrap(),
            item_id: 42,
            matched_listing_id: Some(99),
            matched_price: Some(1234),
            delivered: true,
            accepted_at: None,
            pending_destinations: 0,
            delivery_error: None,
            read_at: None,
            title: Some("Title".to_string()),
            body: Some("Body".to_string()),
            click_url: Some("/item/42".to_string()),
        }
    }

    #[test]
    fn maps_every_field_including_timezone_conversion() {
        let m = model();
        let fired_at_utc = m.fired_at.with_timezone(&chrono::Utc);
        let api = alert_event_to_api(m);
        assert_eq!(api.id, 7);
        assert_eq!(api.alert_id, 3);
        assert_eq!(api.fired_at, fired_at_utc);
        assert_eq!(api.item_id, 42);
        assert_eq!(api.matched_listing_id, Some(99));
        assert_eq!(api.matched_price, Some(1234));
        assert!(api.delivered);
        assert_eq!(api.delivery_error, None);
        assert_eq!(api.read_at, None);
        assert_eq!(api.title, Some("Title".to_string()));
        assert_eq!(api.body, Some("Body".to_string()));
        assert_eq!(api.click_url, Some("/item/42".to_string()));
    }

    #[test]
    fn maps_read_at_when_present() {
        let mut m = model();
        let read_at = chrono::FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 9, 15, 13, 0, 0)
            .unwrap();
        m.read_at = Some(read_at);
        let api = alert_event_to_api(m);
        assert_eq!(api.read_at, Some(read_at.with_timezone(&chrono::Utc)));
    }
}
