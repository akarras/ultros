//! Notification-inbox fire helper.
//!
//! Every alert tracker (`price_alert_tracker`, `list_update_alert_tracker`,
//! `sold_alert`, `undercut_alert`) calls [`record_fire`] instead of writing to
//! `alert_event` directly, so a fire is recorded *and* pushed to the owner's
//! open inbox sockets (via the `notifications` bus in `crate::event`) in one
//! place. REST (`ultros/src/web/api/alerts.rs`) reuses [`alert_event_to_api`]
//! so the websocket push and the paginated history endpoint always agree on
//! the wire shape.

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
    pub delivered: bool,
    pub delivery_error: Option<String>,
}

/// Record an alert fire and broadcast it to the owner's open inbox sockets.
///
/// Never propagates an error: a failed insert or a full/closed notification
/// bus must not abort the delivery path that called this (Discord/webhook
/// dispatch already happened by the time a tracker calls `record_fire`). Both
/// failure modes are logged instead.
pub async fn record_fire(
    db: &UltrosDb,
    notifications: &EventProducer<NotificationEvent>,
    fire: AlertFire<'_>,
) {
    let AlertFire {
        alert_id,
        owner,
        item_id,
        matched_listing_id,
        matched_price,
        title,
        body,
        click_url,
        delivered,
        delivery_error,
    } = fire;
    let inserted = db
        .record_alert_event(NewAlertEvent {
            alert_id,
            item_id,
            matched_listing_id,
            matched_price,
            delivered,
            delivery_error,
            title: title.to_string(),
            body: body.to_string(),
            click_url: click_url.to_string(),
        })
        .await;
    let model = match inserted {
        Ok(model) => model,
        Err(e) => {
            error!("failed to record alert_event for alert {alert_id}: {e}");
            return;
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
