//! Web Push subscription endpoints.
//!
//! Two routes:
//!
//! * `GET  /api/v1/push/vapid-public-key` — hands the browser the base64url
//!   server public key it feeds into `pushManager.subscribe`. 503 if push is
//!   not configured (env vars missing at startup).
//! * `POST /api/v1/push/subscribe` — persists a browser's push subscription
//!   and creates the matching `notification_endpoint` row pointing at it.
//!
//! Direct creation of a `WebPush` endpoint via the generic endpoints CRUD is
//! rejected (see `endpoints::validate_endpoint_method`) because the row is
//! useless without an accompanying `push_subscription`.

use axum::{Json, extract::State};
use hyper::StatusCode;
use ultros_api_types::alert::{
    CreatePushSubscriptionRequest, Endpoint, EndpointMethod, VapidPublicKey,
};
use ultros_db::UltrosDb;

use crate::alerts::delivery::get_web_push_config;
use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;

/// Return the VAPID server public key, base64url-encoded.
///
/// The frontend converts it to a `Uint8Array` and passes it as
/// `applicationServerKey` to `pushManager.subscribe`. If the operator hasn't
/// set the VAPID env vars, return 503 so the frontend can show "push isn't
/// available" instead of trying to subscribe with no key.
pub(crate) async fn get_vapid_public_key()
-> Result<Json<VapidPublicKey>, (StatusCode, &'static str)> {
    match get_web_push_config() {
        Some(cfg) => Ok(Json(VapidPublicKey {
            key: cfg.public_key_b64url.clone(),
        })),
        None => Err((StatusCode::SERVICE_UNAVAILABLE, "Web Push not configured")),
    }
}

/// Persist a browser's Push API subscription and create a matching endpoint row.
///
/// Returns the new [`Endpoint`] so the frontend can immediately add it to its
/// endpoint list without a second fetch.
pub(crate) async fn create_push_subscription(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(req): Json<CreatePushSubscriptionRequest>,
) -> Result<Json<Endpoint>, ApiError> {
    if get_web_push_config().is_none() {
        return Err(ApiError::AnyhowError(anyhow::anyhow!(
            "Web Push not configured"
        )));
    }

    // Basic sanity: the browser MUST give us non-empty endpoint + keys, or the
    // push service will refuse later anyway. Fail fast with a clear message.
    if req.endpoint.is_empty() || req.p256dh.is_empty() || req.auth.is_empty() {
        return Err(ApiError::AnyhowError(anyhow::anyhow!(
            "push subscription missing required fields"
        )));
    }

    let subscription_id = db
        .create_push_subscription(
            user.id as i64,
            &req.endpoint,
            &req.p256dh,
            &req.auth,
            req.user_agent.as_deref(),
        )
        .await
        .map_err(ApiError::from)?;

    let name = endpoint_name_from_user_agent(req.user_agent.as_deref());
    let id = db
        .get_or_create_webpush_endpoint(user.id as i64, subscription_id, &name)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(Endpoint {
        id,
        name,
        method: EndpointMethod::WebPush { subscription_id },
    }))
}

/// Best-effort label derived from `navigator.userAgent`. We're not trying to be
/// a UA parser — just pull out a short hint so the user can tell their devices
/// apart in the endpoints list. Falls back to "Browser" when we can't tell.
fn endpoint_name_from_user_agent(ua: Option<&str>) -> String {
    let Some(ua) = ua else {
        return "Browser".to_string();
    };
    let browser = if ua.contains("Edg/") {
        "Edge"
    } else if ua.contains("OPR/") || ua.contains("Opera") {
        "Opera"
    } else if ua.contains("Firefox/") {
        "Firefox"
    } else if ua.contains("Chrome/") {
        "Chrome"
    } else if ua.contains("Safari/") {
        "Safari"
    } else {
        return "Browser".to_string();
    };
    let os = if ua.contains("Windows") {
        Some("Windows")
    } else if ua.contains("Macintosh") || ua.contains("Mac OS X") {
        Some("macOS")
    } else if ua.contains("Android") {
        Some("Android")
    } else if ua.contains("iPhone") || ua.contains("iPad") || ua.contains("iOS") {
        Some("iOS")
    } else if ua.contains("Linux") {
        Some("Linux")
    } else {
        None
    };
    match os {
        Some(os) => format!("Browser ({browser} on {os})"),
        None => format!("Browser ({browser})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_for_chrome_on_macos() {
        let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";
        assert_eq!(
            endpoint_name_from_user_agent(Some(ua)),
            "Browser (Chrome on macOS)"
        );
    }

    #[test]
    fn label_for_firefox_on_windows() {
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:121.0) Gecko/20100101 Firefox/121.0";
        assert_eq!(
            endpoint_name_from_user_agent(Some(ua)),
            "Browser (Firefox on Windows)"
        );
    }

    #[test]
    fn label_for_safari_on_ios() {
        let ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 Version/17.0 Mobile/15E148 Safari/604.1";
        // iPhone wins for OS even though Safari/ matches the browser check.
        let name = endpoint_name_from_user_agent(Some(ua));
        assert!(name.contains("iOS"), "got {name}");
    }

    #[test]
    fn label_unknown_falls_back() {
        assert_eq!(endpoint_name_from_user_agent(None), "Browser");
        assert_eq!(endpoint_name_from_user_agent(Some("curl/8.0")), "Browser");
    }
}
