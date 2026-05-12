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

pub(crate) async fn create_alert(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(req): Json<CreateAlertRequest>,
) -> Result<Json<Alert>, ApiError> {
    let cooldown = resolve_cooldown_seconds(req.cooldown_seconds);
    let owner = user.id as i64;

    let AlertTrigger::BelowThreshold {
        item_id,
        world_selector,
        price_threshold,
        hq_only,
    } = req.trigger;

    validate_price_threshold(price_threshold)?;

    // Legacy path: `delivery` is required while endpoints are not yet wired up through
    // `endpoint_ids`. Task 11 of the Notification Tier 1 plan migrates this away.
    let delivery = req.delivery.clone().ok_or_else(|| {
        ApiError::from(anyhow::anyhow!(
            "delivery is required (endpoint_ids not yet supported on this handler)"
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
        delivery,
        endpoint_ids: vec![],
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
            endpoint_ids: vec![],
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
        validate_price_threshold(price)?;
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
        return Err(ApiError::from(anyhow::anyhow!(
            "webhook URL must use https"
        )));
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

    // ---------- validate_discord_webhook_url ----------

    #[test]
    fn webhook_url_accepts_canonical_discord_host() {
        assert!(validate_discord_webhook_url("https://discord.com/api/webhooks/1/abc").is_ok());
    }

    #[test]
    fn webhook_url_accepts_all_documented_discord_hosts() {
        for host in [
            "discord.com",
            "discordapp.com",
            "ptb.discord.com",
            "canary.discord.com",
        ] {
            let url = format!("https://{host}/api/webhooks/1/abc");
            assert!(
                validate_discord_webhook_url(&url).is_ok(),
                "expected ok for {url}"
            );
        }
    }

    #[test]
    fn webhook_url_rejects_non_https_scheme() {
        assert!(validate_discord_webhook_url("http://discord.com/api/webhooks/1/abc").is_err());
        assert!(validate_discord_webhook_url("ftp://discord.com/api/webhooks/1/abc").is_err());
    }

    #[test]
    fn webhook_url_rejects_non_discord_host() {
        assert!(validate_discord_webhook_url("https://evil.com/api/webhooks/1/abc").is_err());
        assert!(
            validate_discord_webhook_url("https://discord.com.evil.com/api/webhooks/1/abc")
                .is_err()
        );
    }

    #[test]
    fn webhook_url_rejects_wrong_path_prefix() {
        assert!(validate_discord_webhook_url("https://discord.com/").is_err());
        assert!(validate_discord_webhook_url("https://discord.com/api/").is_err());
        assert!(
            validate_discord_webhook_url("https://discord.com/login?next=/api/webhooks/").is_err()
        );
    }

    #[test]
    fn webhook_url_rejects_garbage_string() {
        assert!(validate_discord_webhook_url("not a url").is_err());
        assert!(validate_discord_webhook_url("").is_err());
    }
}
