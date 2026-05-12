//! Web Push subscription endpoints.
//!
//! The `POST /subscribe` handler lives next to `get_vapid_public_key` so the
//! two routes can share helpers (label parsing, the WebPushConfig accessor).

use axum::Json;
use hyper::StatusCode;
use ultros_api_types::alert::VapidPublicKey;

use crate::alerts::delivery::get_web_push_config;

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
