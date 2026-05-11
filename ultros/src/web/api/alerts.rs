use axum::{
    Json,
    extract::{Path, State},
};
use ultros_api_types::alert::{
    Alert, AlertDelivery, AlertEvent as ApiAlertEvent, AlertTrigger, CreateAlertRequest,
    UpdateAlertRequest,
};
use ultros_db::UltrosDb;

use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;

pub(crate) async fn create_alert(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(req): Json<CreateAlertRequest>,
) -> Result<Json<Alert>, ApiError> {
    let cooldown = req.cooldown_seconds.unwrap_or(3600).clamp(60, 86400);
    let owner = user.id as i64;

    let AlertTrigger::BelowThreshold {
        item_id,
        world_selector,
        price_threshold,
        hq_only,
    } = req.trigger;

    if price_threshold <= 0 {
        return Err(ApiError::from(anyhow::anyhow!(
            "price_threshold must be positive"
        )));
    }

    let (notification_method, notification_config, notification_name): (&str, _, String) =
        match &req.delivery {
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

    // AnySelector is Copy — passed by value here, also Copied into the response below.
    let world_selector_json = serde_json::to_value(world_selector)
        .map_err(|e| ApiError::from(anyhow::anyhow!("invalid world_selector: {}", e)))?;

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

    Ok(Json(Alert {
        id: alert.id,
        trigger: AlertTrigger::BelowThreshold {
            item_id,
            world_selector,
            price_threshold,
            hq_only,
        },
        delivery: req.delivery,
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
            enabled: a.enabled,
            cooldown_seconds: a.cooldown_seconds,
            last_fired_at: a.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
        });
    }
    Ok(Json(out))
}

pub(crate) async fn update_alert(
    State(db): State<UltrosDb>,
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
        if price <= 0 {
            return Err(ApiError::from(anyhow::anyhow!(
                "price_threshold must be positive"
            )));
        }
        db.update_threshold_alert_price(owner, alert_id, price)
            .await
            .map_err(ApiError::from)?;
    }
    Ok(Json(()))
}

pub(crate) async fn delete_alert(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(alert_id): Path<i32>,
) -> Result<Json<()>, ApiError> {
    db.delete_alert_owned_by(user.id as i64, alert_id)
        .await
        .map_err(ApiError::from)?;
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

#[allow(clippy::result_large_err)]
fn validate_discord_webhook_url(url: &str) -> Result<(), ApiError> {
    let parsed = url::Url::parse(url)
        .map_err(|e| ApiError::from(anyhow::anyhow!("invalid webhook URL: {e}")))?;
    if parsed.scheme() != "https" {
        return Err(ApiError::from(anyhow::anyhow!("webhook URL must use https")));
    }
    let host = parsed.host_str().unwrap_or("");
    let allowed = [
        "discord.com",
        "discordapp.com",
        "ptb.discord.com",
        "canary.discord.com",
    ];
    if !allowed.contains(&host) {
        return Err(ApiError::from(anyhow::anyhow!(
            "webhook URL host must be a Discord webhook host"
        )));
    }
    if !parsed.path().starts_with("/api/webhooks/") {
        return Err(ApiError::from(anyhow::anyhow!(
            "webhook URL path must start with /api/webhooks/"
        )));
    }
    Ok(())
}
