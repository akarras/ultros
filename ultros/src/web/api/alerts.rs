use axum::{
    Json,
    extract::{Path, State},
};
use ultros_api_types::alert::{
    Alert, AlertDelivery, AlertEvent as ApiAlertEvent, AlertTrigger, CreateAlertRequest,
    ResendResult, UpdateAlertRequest,
};
use ultros_api_types::list::ListPermission;
use ultros_db::UltrosDb;

use crate::event::{EventSenders, EventType};
use crate::web::api::endpoint_validation::validate_discord_webhook_url;
use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;

/// Default cooldown when the user doesn't supply one (1 hour).
pub(crate) const DEFAULT_COOLDOWN_SECONDS: i32 = 3600;
/// Minimum cooldown a user can request (1 minute) — prevents spam.
pub(crate) const MIN_COOLDOWN_SECONDS: i32 = 60;
/// Maximum cooldown a user can request (1 day).
pub(crate) const MAX_COOLDOWN_SECONDS: i32 = 86400;

/// Resolve a user-supplied cooldown into the actual cooldown stored in the DB.
/// Missing → default (1h). Out-of-range → clamped to [60s, 86400s].
pub(crate) fn resolve_cooldown_seconds(requested: Option<i32>) -> i32 {
    requested
        .unwrap_or(DEFAULT_COOLDOWN_SECONDS)
        .clamp(MIN_COOLDOWN_SECONDS, MAX_COOLDOWN_SECONDS)
}

/// Validate a price threshold from a user request. Returns `Err` when zero or negative.
#[allow(clippy::result_large_err)]
pub(crate) fn validate_price_threshold(price_threshold: i32) -> Result<(), ApiError> {
    if price_threshold <= 0 {
        Err(ApiError::from(anyhow::anyhow!(
            "price_threshold must be positive"
        )))
    } else {
        Ok(())
    }
}

#[allow(clippy::result_large_err)]
pub(crate) fn validate_margin_percent(margin_percent: i32) -> Result<(), ApiError> {
    if !(0..=200).contains(&margin_percent) {
        Err(ApiError::from(anyhow::anyhow!(
            "margin_percent must be between 0 and 200"
        )))
    } else {
        Ok(())
    }
}

pub(crate) async fn create_alert(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Json(req): Json<CreateAlertRequest>,
) -> Result<Json<Alert>, ApiError> {
    let cooldown = resolve_cooldown_seconds(req.cooldown_seconds);
    let owner = user.id as i64;

    let (item_id, world_selector, price_threshold, hq_only) = match req.trigger {
        AlertTrigger::BelowThreshold {
            item_id,
            world_selector,
            price_threshold,
            hq_only,
        } => (item_id, world_selector, price_threshold, hq_only),
        AlertTrigger::ListItemThreshold { list_id } => {
            return create_list_threshold_alert_handler(
                &db, &senders, owner, list_id, cooldown, &req,
            )
            .await;
        }
        AlertTrigger::RetainerUndercut { margin_percent } => {
            return create_retainer_undercut_alert_handler(
                &db,
                &senders,
                owner,
                margin_percent,
                cooldown,
                &req,
            )
            .await;
        }
        AlertTrigger::ListUpdate { list_id } => {
            return create_list_update_alert_handler(&db, &senders, owner, list_id, cooldown, &req)
                .await;
        }
    };

    validate_price_threshold(price_threshold)?;

    // AnySelector is Copy — passed by value here, also Copied into the response below.
    let world_selector_json = serde_json::to_value(world_selector)
        .map_err(|e| ApiError::from(anyhow::anyhow!("invalid world_selector: {}", e)))?;

    // Two paths:
    //  1. endpoint_ids non-empty → create alert + bind to those endpoints (preferred)
    //  2. endpoint_ids empty AND delivery provided → legacy path: create endpoint inline,
    //     bind one rule
    //  3. neither → error
    if !req.endpoint_ids.is_empty() {
        // Verify all endpoints belong to this user before creating the alert.
        for &eid in &req.endpoint_ids {
            db.get_endpoint_owned_by(owner, eid)
                .await
                .map_err(ApiError::from)?;
        }
        let alert = db
            .create_threshold_alert_without_endpoint(
                owner,
                item_id,
                world_selector_json,
                price_threshold,
                hq_only,
                cooldown,
            )
            .await
            .map_err(ApiError::from)?;
        db.set_alert_rules(owner, alert.id, &req.endpoint_ids)
            .await
            .map_err(ApiError::from)?;
        let _ = senders.alerts.send(EventType::added(alert.clone()));

        return Ok(Json(Alert {
            id: alert.id,
            trigger: AlertTrigger::BelowThreshold {
                item_id,
                world_selector,
                price_threshold,
                hq_only,
            },
            // deprecated fallback; real delivery is described by endpoint_ids
            delivery: AlertDelivery::DiscordDm,
            endpoint_ids: req.endpoint_ids,
            enabled: alert.enabled,
            cooldown_seconds: alert.cooldown_seconds,
            last_fired_at: alert.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
        }));
    }

    // Legacy path: `delivery` is required when no endpoint_ids are provided.
    let delivery = req.delivery.ok_or_else(|| {
        ApiError::from(anyhow::anyhow!(
            "either endpoint_ids or delivery must be supplied"
        ))
    })?;

    let (notification_method, notification_config, notification_name): (&str, _, String) =
        match &delivery {
            AlertDelivery::DiscordDm => (
                "DiscordDm",
                serde_json::json!({ "user_id": owner }),
                format!("DM to {}", user.name),
            ),
            AlertDelivery::Webhook { url } => {
                validate_discord_webhook_url(url)?;
                (
                    "Webhook",
                    serde_json::json!({ "url": url }),
                    format!("Webhook to {url}"),
                )
            }
        };

    let alert = db
        .create_threshold_alert(
            owner,
            item_id,
            world_selector_json,
            price_threshold,
            hq_only,
            cooldown,
            notification_method,
            notification_config,
            &notification_name,
        )
        .await
        .map_err(ApiError::from)?;
    let _ = senders.alerts.send(EventType::added(alert.clone()));

    let endpoint_ids = db
        .list_endpoint_ids_for_alert(alert.id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(Alert {
        id: alert.id,
        trigger: AlertTrigger::BelowThreshold {
            item_id,
            world_selector,
            price_threshold,
            hq_only,
        },
        delivery,
        endpoint_ids,
        enabled: alert.enabled,
        cooldown_seconds: alert.cooldown_seconds,
        last_fired_at: alert.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
    }))
}

/// Handle `create_alert` for the `ListItemThreshold` variant. Split out because
/// the body has its own permission/endpoint flow distinct from the item-scoped
/// path, and a single big function gets unwieldy.
async fn create_list_threshold_alert_handler(
    db: &UltrosDb,
    senders: &EventSenders,
    owner: i64,
    list_id: i32,
    cooldown: i32,
    req: &CreateAlertRequest,
) -> Result<Json<Alert>, ApiError> {
    // List-scoped alerts only support the endpoint_ids delivery shape — the
    // legacy `delivery` field doesn't carry enough information to bind a
    // single user to many items.
    if req.endpoint_ids.is_empty() {
        return Err(ApiError::from(anyhow::anyhow!(
            "list-scoped alerts require endpoint_ids"
        )));
    }

    // Permission gate: caller must have at least Read on the list. Sharing a
    // list with View permission is sufficient to subscribe; we don't require
    // ownership because Tier 4 is explicitly about shared lists.
    let permission = db
        .get_permission(list_id, owner)
        .await
        .map_err(ApiError::from)?;
    if permission < ListPermission::Read {
        return Err(ApiError::from(anyhow::anyhow!(
            "insufficient permission on list"
        )));
    }

    // Verify all endpoints belong to this user before creating the alert.
    for &eid in &req.endpoint_ids {
        db.get_endpoint_owned_by(owner, eid)
            .await
            .map_err(ApiError::from)?;
    }

    let alert = db
        .create_list_threshold_alert(owner, list_id, cooldown, &req.endpoint_ids)
        .await
        .map_err(ApiError::from)?;
    let _ = senders.alerts.send(EventType::added(alert.clone()));

    Ok(Json(Alert {
        id: alert.id,
        trigger: AlertTrigger::ListItemThreshold { list_id },
        // Deprecated fallback. Real delivery is endpoint_ids; this field only
        // exists for the older clients that pre-date the endpoints framework.
        delivery: AlertDelivery::DiscordDm,
        endpoint_ids: req.endpoint_ids.clone(),
        enabled: alert.enabled,
        cooldown_seconds: alert.cooldown_seconds,
        last_fired_at: alert.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
    }))
}

async fn create_retainer_undercut_alert_handler(
    db: &UltrosDb,
    senders: &EventSenders,
    owner: i64,
    margin_percent: i32,
    cooldown: i32,
    req: &CreateAlertRequest,
) -> Result<Json<Alert>, ApiError> {
    validate_margin_percent(margin_percent)?;
    if req.endpoint_ids.is_empty() {
        return Err(ApiError::from(anyhow::anyhow!(
            "retainer undercut alerts require endpoint_ids"
        )));
    }

    let (alert, undercut) = db
        .create_retainer_undercut_alert(owner, margin_percent, cooldown, &req.endpoint_ids)
        .await
        .map_err(ApiError::from)?;
    let _ = senders.alerts.send(EventType::added(alert.clone()));
    let _ = senders.retainer_undercut.send(EventType::added(undercut));

    Ok(Json(Alert {
        id: alert.id,
        trigger: AlertTrigger::RetainerUndercut { margin_percent },
        delivery: AlertDelivery::DiscordDm,
        endpoint_ids: req.endpoint_ids.clone(),
        enabled: alert.enabled,
        cooldown_seconds: alert.cooldown_seconds,
        last_fired_at: alert.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
    }))
}

async fn create_list_update_alert_handler(
    db: &UltrosDb,
    senders: &EventSenders,
    owner: i64,
    list_id: i32,
    cooldown: i32,
    req: &CreateAlertRequest,
) -> Result<Json<Alert>, ApiError> {
    if req.endpoint_ids.is_empty() {
        return Err(ApiError::from(anyhow::anyhow!(
            "list update alerts require endpoint_ids"
        )));
    }

    let permission = db
        .get_permission(list_id, owner)
        .await
        .map_err(ApiError::from)?;
    if permission < ListPermission::Read {
        return Err(ApiError::from(anyhow::anyhow!(
            "insufficient permission on list"
        )));
    }

    let alert = db
        .create_list_update_alert(owner, list_id, cooldown, &req.endpoint_ids)
        .await
        .map_err(ApiError::from)?;
    let _ = senders.alerts.send(EventType::added(alert.clone()));

    Ok(Json(Alert {
        id: alert.id,
        trigger: AlertTrigger::ListUpdate { list_id },
        delivery: AlertDelivery::DiscordDm,
        endpoint_ids: req.endpoint_ids.clone(),
        enabled: alert.enabled,
        cooldown_seconds: alert.cooldown_seconds,
        last_fired_at: alert.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
    }))
}

pub(crate) async fn list_alerts(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<Alert>>, ApiError> {
    let rows = db
        .get_user_threshold_alerts(user.id as i64)
        .await
        .map_err(ApiError::from)?;
    let mut out = Vec::with_capacity(rows.len());
    for (a, t) in rows {
        let world_selector = serde_json::from_value(t.world_selector.clone())
            .map_err(|e| ApiError::from(anyhow::anyhow!("bad world_selector in db: {}", e)))?;
        let endpoint_ids = db
            .list_endpoint_ids_for_alert(a.id)
            .await
            .map_err(ApiError::from)?;
        let delivery = match db
            .get_first_endpoint_for_alert(a.id)
            .await
            .map_err(ApiError::from)?
        {
            Some(e) if e.method == "Webhook" => {
                let url = e
                    .config
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                AlertDelivery::Webhook { url }
            }
            _ => AlertDelivery::DiscordDm,
        };
        out.push(Alert {
            id: a.id,
            trigger: AlertTrigger::BelowThreshold {
                item_id: t.item_id,
                world_selector,
                price_threshold: t.price_threshold,
                hq_only: t.hq_only,
            },
            delivery,
            endpoint_ids,
            enabled: a.enabled,
            cooldown_seconds: a.cooldown_seconds,
            last_fired_at: a.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
        });
    }

    // Union with the user's list-threshold alerts. The shape converges on the
    // same Alert struct — only the trigger variant differs.
    let list_rows = db
        .get_user_list_threshold_alerts(user.id as i64)
        .await
        .map_err(ApiError::from)?;
    for (a, t) in list_rows {
        let endpoint_ids = db
            .list_endpoint_ids_for_alert(a.id)
            .await
            .map_err(ApiError::from)?;
        out.push(Alert {
            id: a.id,
            trigger: AlertTrigger::ListItemThreshold { list_id: t.list_id },
            // Deprecated; new clients use endpoint_ids.
            delivery: AlertDelivery::DiscordDm,
            endpoint_ids,
            enabled: a.enabled,
            cooldown_seconds: a.cooldown_seconds,
            last_fired_at: a.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
        });
    }

    let retainer_rows = db
        .get_user_retainer_undercut_alerts(user.id as i64)
        .await
        .map_err(ApiError::from)?;
    for (a, t) in retainer_rows {
        let endpoint_ids = db
            .list_endpoint_ids_for_alert(a.id)
            .await
            .map_err(ApiError::from)?;
        out.push(Alert {
            id: a.id,
            trigger: AlertTrigger::RetainerUndercut {
                margin_percent: t.margin_percent,
            },
            delivery: AlertDelivery::DiscordDm,
            endpoint_ids,
            enabled: a.enabled,
            cooldown_seconds: a.cooldown_seconds,
            last_fired_at: a.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
        });
    }

    let update_rows = db
        .get_user_list_update_alerts(user.id as i64)
        .await
        .map_err(ApiError::from)?;
    for (a, t) in update_rows {
        let endpoint_ids = db
            .list_endpoint_ids_for_alert(a.id)
            .await
            .map_err(ApiError::from)?;
        out.push(Alert {
            id: a.id,
            trigger: AlertTrigger::ListUpdate { list_id: t.list_id },
            delivery: AlertDelivery::DiscordDm,
            endpoint_ids,
            enabled: a.enabled,
            cooldown_seconds: a.cooldown_seconds,
            last_fired_at: a.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
        });
    }
    Ok(Json(out))
}

pub(crate) async fn update_alert(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Path(alert_id): Path<i32>,
    Json(req): Json<UpdateAlertRequest>,
) -> Result<Json<()>, ApiError> {
    let owner = user.id as i64;
    if let Some(enabled) = req.enabled {
        db.set_alert_enabled(owner, alert_id, enabled)
            .await
            .map_err(ApiError::from)?;
    }
    if let Some(price) = req.price_threshold {
        validate_price_threshold(price)?;
        db.update_threshold_alert_price(owner, alert_id, price)
            .await
            .map_err(ApiError::from)?;
    }
    if let Some(cooldown) = req.cooldown_seconds {
        db.set_alert_cooldown(owner, alert_id, resolve_cooldown_seconds(Some(cooldown)))
            .await
            .map_err(ApiError::from)?;
    }
    if let Some(endpoint_ids) = req.endpoint_ids {
        if endpoint_ids.is_empty() {
            return Err(ApiError::from(anyhow::anyhow!(
                "alerts require at least one endpoint"
            )));
        }
        db.set_alert_rules(owner, alert_id, &endpoint_ids)
            .await
            .map_err(ApiError::from)?;
    }
    if let Some(alert) = db.get_alert(alert_id).await.map_err(ApiError::from)? {
        let _ = senders.alerts.send(EventType::updated(alert));
    }
    Ok(Json(()))
}

pub(crate) async fn delete_alert(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Path(alert_id): Path<i32>,
) -> Result<Json<()>, ApiError> {
    let existing = db
        .get_alert(alert_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::from(anyhow::anyhow!("alert not found")))?;
    let retainer_alerts = db
        .get_retainer_alerts_for_related_alert_id(alert_id)
        .await
        .map_err(ApiError::from)?;
    db.delete_alert_owned_by(user.id as i64, alert_id)
        .await
        .map_err(ApiError::from)?;
    let _ = senders.alerts.send(EventType::removed(existing));
    for retainer_alert in retainer_alerts {
        let _ = senders
            .retainer_undercut
            .send(EventType::removed(retainer_alert));
    }
    Ok(Json(()))
}

pub(crate) async fn list_alert_events(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<ApiAlertEvent>>, ApiError> {
    let rows = db
        .get_recent_alert_events_for_user(user.id as i64, 50)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(
        rows.into_iter()
            .map(|r| ApiAlertEvent {
                id: r.id,
                alert_id: r.alert_id,
                fired_at: r.fired_at.with_timezone(&chrono::Utc),
                item_id: r.item_id,
                matched_listing_id: r.matched_listing_id,
                matched_price: r.matched_price,
                delivered: r.delivered,
                delivery_error: r.delivery_error,
            })
            .collect(),
    ))
}

/// Resend an alert event through every endpoint linked to its alert. Returns
/// `ResendResult { delivered, error }` rather than bailing out — the UI shows
/// per-event status, so a soft failure (no endpoints, all endpoints errored) is
/// reported as `delivered: false` with the last error message captured.
///
/// Path: `POST /api/v1/alerts/events/{id}/resend`.
pub(crate) async fn resend_alert_event(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(event_id): Path<i64>,
) -> Result<Json<ResendResult>, ApiError> {
    let event = db
        .get_alert_event_by_id_owned_by(user.id as i64, event_id)
        .await
        .map_err(ApiError::from)?;
    let endpoints = db
        .get_notification_endpoints_for_alert(event.alert_id)
        .await
        .map_err(ApiError::from)?;
    if endpoints.is_empty() {
        return Ok(Json(ResendResult {
            delivered: false,
            error: Some("alert has no endpoints".into()),
        }));
    }
    let serenity_ctx = crate::alerts::delivery::get_serenity_ctx();
    let title = "Ultros alert (resend)";
    let body = format!(
        "Resending alert for item {} (matched price: {:?})",
        event.item_id, event.matched_price
    );
    let mut last_err: Option<String> = None;
    let mut any_ok = false;
    let owner = user.id as i64;
    for endpoint in endpoints {
        // Defense in depth: today `set_alert_rules` is the only path that links endpoints
        // to alerts and it verifies ownership at link time, but skipping the check here
        // would silently deliver through a foreign endpoint if a future code path bypassed
        // that invariant. Endpoints that don't belong to the caller are treated as missing.
        if endpoint.user_id != owner {
            last_err = Some(format!(
                "endpoint {} not owned by caller; skipped",
                endpoint.id
            ));
            continue;
        }
        let needs_ctx = matches!(endpoint.method.as_str(), "DiscordDm" | "DiscordChannel");
        let result = if needs_ctx {
            match serenity_ctx.as_ref() {
                Some(ctx) => {
                    crate::alerts::delivery::deliver_to_endpoint(&endpoint, title, &body, &db, ctx)
                        .await
                }
                None => Err(anyhow::anyhow!("Discord client not ready")),
            }
        } else {
            crate::alerts::delivery::deliver_non_discord_endpoint(&endpoint, title, &body, &db)
                .await
        };
        match result {
            Ok(()) => any_ok = true,
            Err(e) => last_err = Some(format!("{e}")),
        }
    }
    Ok(Json(ResendResult {
        delivered: any_ok,
        error: last_err,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- resolve_cooldown_seconds ----------

    #[test]
    fn cooldown_default_when_unset() {
        assert_eq!(resolve_cooldown_seconds(None), DEFAULT_COOLDOWN_SECONDS);
    }

    #[test]
    fn cooldown_passes_in_range_values_through() {
        assert_eq!(resolve_cooldown_seconds(Some(60)), 60);
        assert_eq!(resolve_cooldown_seconds(Some(3600)), 3600);
        assert_eq!(resolve_cooldown_seconds(Some(86400)), 86400);
        assert_eq!(resolve_cooldown_seconds(Some(7200)), 7200);
    }

    #[test]
    fn cooldown_clamps_to_min_when_below_60() {
        assert_eq!(resolve_cooldown_seconds(Some(0)), 60);
        assert_eq!(resolve_cooldown_seconds(Some(59)), 60);
        assert_eq!(resolve_cooldown_seconds(Some(-1)), 60);
        assert_eq!(resolve_cooldown_seconds(Some(i32::MIN)), 60);
    }

    #[test]
    fn cooldown_clamps_to_max_when_above_86400() {
        assert_eq!(resolve_cooldown_seconds(Some(86401)), 86400);
        assert_eq!(resolve_cooldown_seconds(Some(1_000_000)), 86400);
        assert_eq!(resolve_cooldown_seconds(Some(i32::MAX)), 86400);
    }

    // ---------- validate_price_threshold ----------

    #[test]
    fn price_threshold_accepts_positive() {
        assert!(validate_price_threshold(1).is_ok());
        assert!(validate_price_threshold(100).is_ok());
        assert!(validate_price_threshold(i32::MAX).is_ok());
    }

    #[test]
    fn price_threshold_rejects_zero_and_negative() {
        assert!(validate_price_threshold(0).is_err());
        assert!(validate_price_threshold(-1).is_err());
        assert!(validate_price_threshold(i32::MIN).is_err());
    }
}
