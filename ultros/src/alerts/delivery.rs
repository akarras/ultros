use anyhow::{Result, anyhow};
use poise::serenity_prelude::{
    self, Color, CreateAllowedMentions, CreateEmbed, CreateMessage, UserId,
};
use serde::Deserialize;
use std::sync::{Arc, OnceLock};
use tracing::error;
use ultros_db::UltrosDb;

/// Process-wide handle to the running Discord client's `serenity::Context`.
///
/// The bot owns the live context, but web handlers (`/test`, `/resend`) also need to send
/// Discord messages. The Discord setup hook calls [`set_serenity_ctx`] once during startup;
/// any later caller can [`get_serenity_ctx`] it back out. Returns `None` before the bot has
/// finished initializing — handlers should map that to a user-facing error.
static SERENITY_CTX: OnceLock<Arc<serenity_prelude::Context>> = OnceLock::new();

/// Install the global serenity context. Called once during Discord framework setup.
/// Subsequent calls are ignored (OnceLock semantics).
pub fn set_serenity_ctx(ctx: serenity_prelude::Context) {
    let _ = SERENITY_CTX.set(Arc::new(ctx));
}

/// Fetch the global serenity context, if the bot has finished initializing.
pub(crate) fn get_serenity_ctx() -> Option<Arc<serenity_prelude::Context>> {
    SERENITY_CTX.get().cloned()
}

/// VAPID configuration required to sign + send Web Push messages.
///
/// Operators must generate the keypair offline (one-shot, then keep the private key
/// secret) — we do **not** generate it at runtime, because rotating keys would
/// invalidate every existing subscription. See `docs/push.md` for the openssl
/// recipe.
#[derive(Debug, Clone)]
pub struct WebPushConfig {
    /// Base64url-encoded uncompressed P-256 public key (no padding). Served verbatim
    /// to the frontend, which decodes it to a `Uint8Array` for `applicationServerKey`.
    pub public_key_b64url: String,
    /// EC private key used for VAPID signing. Accepts either PEM (including env
    /// values with escaped `\n`) or the base64url private key produced by common
    /// `web-push generate-vapid-keys` tooling.
    pub private_key_pem: String,
    /// `mailto:` URI placed in the JWT's `sub` claim. Some push services reject
    /// non-`mailto:` values.
    pub contact_email: String,
}

/// Process-wide Web Push configuration. Mirrors the [`SERENITY_CTX`] bridge: set
/// once at startup from env vars, read by both the public-key endpoint and the
/// delivery path. `None` means push is disabled (env vars absent) — handlers map
/// that to a 503.
static WEB_PUSH_CONFIG: OnceLock<WebPushConfig> = OnceLock::new();

/// Install the global Web Push config. Idempotent — second call wins nothing.
pub fn set_web_push_config(cfg: WebPushConfig) {
    let _ = WEB_PUSH_CONFIG.set(cfg);
}

/// Fetch the global Web Push config, if one was installed at startup.
pub(crate) fn get_web_push_config() -> Option<&'static WebPushConfig> {
    WEB_PUSH_CONFIG.get()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "method")]
pub(crate) enum EndpointConfig {
    #[serde(rename = "DiscordChannel")]
    DiscordChannel { channel_id: i64 },
    #[serde(rename = "DiscordDm")]
    DiscordDm { user_id: i64 },
    #[serde(rename = "Webhook")]
    Webhook { url: String },
    #[serde(rename = "WebPush")]
    WebPush { subscription_id: i32 },
}

/// Parse a notification endpoint row's `(method, config)` pair into a typed [`EndpointConfig`].
///
/// The DB stores `method` as a separate column and `config` as a JSON object missing the
/// discriminator — this helper splices the discriminator in so `serde(tag = "method")` can
/// deserialize the result.
pub(crate) fn parse_endpoint_config(
    method: &str,
    config: &serde_json::Value,
) -> Result<EndpointConfig> {
    let mut config_obj =
        serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(config.clone())
            .unwrap_or_default();
    config_obj.insert(
        "method".to_string(),
        serde_json::Value::String(method.to_string()),
    );
    serde_json::from_value(serde_json::Value::Object(config_obj))
        .map_err(|e| anyhow!("bad endpoint config: {e}"))
}

/// Deliver a single message to one endpoint. Returns `Ok(())` on success.
///
/// Used by [`dispatch_alert`] (fan-out from the price-alert tracker) and by the web handlers
/// for endpoint test + alert-event resend. The `_db` arg is unused today but kept in the
/// signature so future endpoint methods (e.g. ones that need to look up retainer info) can
/// be added without rippling the call sites.
///
/// `click_url` is the in-app path a Web Push notification opens when clicked
/// (e.g. `/retainers/undercuts` for undercut alerts); use `/alerts` when no
/// more specific destination applies. Other endpoint methods ignore it — their
/// bodies already carry full links.
pub(crate) async fn deliver_to_endpoint(
    endpoint: &ultros_db::entity::notification_endpoint::Model,
    title: &str,
    body: &str,
    click_url: &str,
    db: &UltrosDb,
    ctx: &serenity_prelude::Context,
) -> Result<()> {
    let parsed = parse_endpoint_config(&endpoint.method, &endpoint.config)?;
    match parsed {
        EndpointConfig::DiscordChannel { channel_id } => {
            send_to_channel(channel_id, title, body, ctx).await
        }
        EndpointConfig::DiscordDm { user_id } => send_dm(user_id, title, body, ctx).await,
        EndpointConfig::Webhook { url } => send_webhook(&url, title, body).await,
        EndpointConfig::WebPush { subscription_id } => {
            let cfg = get_web_push_config()
                .ok_or_else(|| anyhow!("web push not configured on this deployment"))?;
            send_webpush(subscription_id, title, body, click_url, db, cfg).await
        }
    }
}

/// Deliver to a non-Discord endpoint without a live serenity context. Used by
/// the web `test` handler when the bot hasn't connected yet — Webhook/WebPush
/// don't need it, so failing those tests on an unrelated dependency was a bug.
/// Errors when called against a Discord endpoint method.
pub(crate) async fn deliver_non_discord_endpoint(
    endpoint: &ultros_db::entity::notification_endpoint::Model,
    title: &str,
    body: &str,
    click_url: &str,
    db: &UltrosDb,
) -> Result<()> {
    let parsed = parse_endpoint_config(&endpoint.method, &endpoint.config)?;
    match parsed {
        EndpointConfig::DiscordChannel { .. } | EndpointConfig::DiscordDm { .. } => {
            Err(anyhow!("Discord endpoints require the bot to be connected"))
        }
        EndpointConfig::Webhook { url } => send_webhook(&url, title, body).await,
        EndpointConfig::WebPush { subscription_id } => {
            let cfg = get_web_push_config()
                .ok_or_else(|| anyhow!("web push not configured on this deployment"))?;
            send_webpush(subscription_id, title, body, click_url, db, cfg).await
        }
    }
}

/// Whether a Discord API rejection can ever succeed on a later retry.
///
/// Discord answers a failed REST call with an HTTP status plus a numeric JSON
/// error code. A handful of those codes describe a destination that is simply
/// *gone* — retrying them every time an alert fires produces nothing but error
/// spam while the owner silently receives no alerts at all.
///
/// Codes (<https://discord.com/developers/docs/topics/opcodes-and-status-codes>):
/// - `10003` Unknown Channel — the channel was deleted.
/// - `10013` Unknown User — the DM target no longer exists.
/// - `50001` Missing Access — the bot was removed from the guild/channel.
/// - `50007` Cannot send messages to this user — DMs closed.
/// - `50013` Missing Permissions — send permission revoked on the channel.
///
/// Everything else (rate limits, 5xx, transport errors) is treated as transient
/// so a Discord outage never disables a working endpoint. `-1` is serenity's
/// placeholder when the error body failed to decode, which tells us nothing —
/// also transient.
fn is_permanent_discord_failure(status: u16, discord_code: isize) -> bool {
    // A 5xx is Discord's problem, never the destination's — regardless of the
    // code it happens to carry.
    if status >= 500 {
        return false;
    }
    matches!(discord_code, 10003 | 10013 | 50001 | 50007 | 50013)
}

/// Pull a permanent-failure reason out of an error returned by
/// [`deliver_to_endpoint`], if the underlying cause was Discord rejecting the
/// destination for good.
///
/// The delivery helpers surface serenity errors through `anyhow`, so walk the
/// source chain rather than matching only the top-level error.
pub(crate) fn permanent_failure_reason(err: &anyhow::Error) -> Option<String> {
    use poise::serenity_prelude::HttpError;

    for cause in err.chain() {
        let Some(serenity_prelude::Error::Http(HttpError::UnsuccessfulRequest(resp))) =
            cause.downcast_ref::<serenity_prelude::Error>()
        else {
            continue;
        };
        if is_permanent_discord_failure(resp.status_code.as_u16(), resp.error.code) {
            return Some(resp.error.message.clone());
        }
    }
    None
}

/// Outcome of fanning an alert out across its notification endpoints.
pub(crate) enum DispatchOutcome {
    /// At least one endpoint accepted the message.
    Delivered,
    /// Nothing delivered, but the failures look transient — the caller should
    /// still try any legacy fallback destinations.
    TransientFailure(anyhow::Error),
    /// Nothing delivered and every failure was permanent (or the alert has no
    /// deliverable endpoints left because they were all disabled). Retrying —
    /// including via the legacy fallback, which points at the same dead Discord
    /// channels — is pointless, so the caller should record the reason quietly
    /// rather than reporting a new error every fire.
    PermanentFailure(String),
}

/// Look up all deliverable notification endpoints for an alert and dispatch the
/// message via each.
///
/// Endpoints that Discord rejects permanently are disabled as a side effect, so
/// the next fire skips them entirely. A successful delivery clears any
/// previously recorded failure.
pub(crate) async fn dispatch_alert_detailed(
    alert_id: i32,
    title: &str,
    body: &str,
    click_url: &str,
    db: &UltrosDb,
    ctx: &serenity_prelude::Context,
) -> DispatchOutcome {
    let endpoints = match db.get_notification_endpoints_for_alert(alert_id).await {
        Ok(e) => e,
        Err(e) => return DispatchOutcome::TransientFailure(e),
    };

    if endpoints.is_empty() {
        // Either the alert never had rules, or every endpoint it had has been
        // disabled for a permanent failure. Both are steady states that a retry
        // cannot change, so don't keep raising them as errors.
        return DispatchOutcome::PermanentFailure(format!(
            "alert {alert_id} has no deliverable notification endpoints"
        ));
    }

    let mut last_err: Option<anyhow::Error> = None;
    let mut permanent_reason: Option<String> = None;
    let mut any_ok = false;
    let mut any_transient = false;

    for endpoint in endpoints {
        match deliver_to_endpoint(&endpoint, title, body, click_url, db, ctx).await {
            Ok(()) => {
                any_ok = true;
                // Only touch the DB when there is actually stale failure state
                // to clear — the healthy path is the common one and shouldn't
                // pay a write per alert fire.
                if (endpoint.disabled_at.is_some() || endpoint.last_error.is_some())
                    && let Err(e) = db.clear_endpoint_delivery_failure(endpoint.id).await
                {
                    error!(
                        "failed to clear delivery failure for endpoint {}: {e}",
                        endpoint.id
                    );
                }
            }
            Err(e) => {
                match permanent_failure_reason(&e) {
                    Some(reason) => {
                        // Log at warn: this is an expected steady state we are
                        // acting on, not an unhandled error, and it should stop
                        // paging via the error reporter.
                        tracing::warn!(
                            "disabling endpoint {} for alert {alert_id}: {reason}",
                            endpoint.id
                        );
                        if let Err(e) = db
                            .disable_endpoint_for_delivery_failure(endpoint.id, &reason)
                            .await
                        {
                            error!("failed to disable endpoint {}: {e}", endpoint.id);
                        }
                        permanent_reason.get_or_insert(reason);
                    }
                    None => {
                        error!("delivery failed for alert {alert_id}: {e}");
                        any_transient = true;
                    }
                }
                last_err = Some(e);
            }
        }
    }

    if any_ok {
        DispatchOutcome::Delivered
    } else if any_transient || permanent_reason.is_none() {
        DispatchOutcome::TransientFailure(
            last_err.unwrap_or_else(|| anyhow!("no deliveries succeeded")),
        )
    } else {
        DispatchOutcome::PermanentFailure(permanent_reason.unwrap_or_default())
    }
}

/// Look up all notification endpoints for an alert and dispatch the message via each.
/// Returns Ok(()) if at least one delivered; Err describing the last failure otherwise.
pub(crate) async fn dispatch_alert(
    alert_id: i32,
    title: &str,
    body: &str,
    click_url: &str,
    db: &UltrosDb,
    ctx: &serenity_prelude::Context,
) -> Result<()> {
    match dispatch_alert_detailed(alert_id, title, body, click_url, db, ctx).await {
        DispatchOutcome::Delivered => Ok(()),
        DispatchOutcome::TransientFailure(e) => Err(e),
        DispatchOutcome::PermanentFailure(reason) => Err(anyhow!("{reason}")),
    }
}

async fn send_to_channel(
    channel_id: i64,
    title: &str,
    body: &str,
    ctx: &serenity_prelude::Context,
) -> Result<()> {
    let channel_id = serenity_prelude::ChannelId::new(channel_id as u64);
    channel_id
        .send_message(
            ctx,
            CreateMessage::new()
                .embed(
                    CreateEmbed::new()
                        .color(Color::from_rgb(0, 200, 80))
                        .title(title)
                        .description(body),
                )
                .allowed_mentions(CreateAllowedMentions::new()),
        )
        .await?;
    Ok(())
}

async fn send_dm(
    user_id: i64,
    title: &str,
    body: &str,
    ctx: &serenity_prelude::Context,
) -> Result<()> {
    let user_id = UserId::new(user_id as u64);
    let dm = user_id.create_dm_channel(ctx).await?;
    dm.send_message(
        ctx,
        CreateMessage::new()
            .embed(
                CreateEmbed::new()
                    .color(Color::from_rgb(0, 200, 80))
                    .title(title)
                    .description(body),
            )
            .allowed_mentions(CreateAllowedMentions::new()),
    )
    .await?;
    Ok(())
}

/// Build the JSON body the service worker reads out of `event.data.json()`.
/// `click_url` becomes `data.url`, which `notificationclick` opens — an alert
/// that hardcodes this loses the user's actual destination.
fn build_push_payload(title: &str, body: &str, click_url: &str) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&serde_json::json!({
        "title": title,
        "body": body,
        "url": click_url,
    }))?)
}

/// A subscription is untrusted input, including subscriptions saved before this
/// policy existed. Only browser push services may receive our signed requests.
/// Keep paths and queries opaque: providers can change their token formats.
/// See `docs/push.md` for the supported providers and the allowlist tradeoff.
pub(crate) fn validate_web_push_endpoint(endpoint: &str) -> Result<url::Url, &'static str> {
    let url = url::Url::parse(endpoint).map_err(|_| "invalid push endpoint URL")?;
    if url.scheme() != "https" || url.port_or_known_default() != Some(443) {
        return Err("push endpoint must use HTTPS on port 443");
    }
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("push endpoint must not contain credentials or a fragment");
    }
    let host = url.host_str().ok_or("push endpoint must have a host")?;
    let supported = matches!(
        host,
        "fcm.googleapis.com" | "updates.push.services.mozilla.com"
    ) || host.ends_with(".push.apple.com")
        || host.ends_with(".notify.windows.com");
    if !supported {
        return Err("push endpoint must belong to a supported browser push service");
    }
    Ok(url)
}

fn web_push_http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        // Even a supported provider must not redirect our signed request to an
        // arbitrary URL. A redirect is a delivery failure, never a new target.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()?)
}

/// Send a Web Push notification to a single subscription. Body is JSON-encoded
/// `{title, body, url}` — the service worker decodes that in its `push` handler.
///
/// On `EndpointNotFound`/`EndpointNotValid` (the push service signaling the
/// subscription has been revoked), soft-delete the row so we don't keep trying
/// to send to a dead endpoint. Other errors propagate to the caller as-is,
/// which lets `alert_event.delivery_error` capture the failure.
///
/// The HTTP request is constructed by `web-push`'s `request_builder` (which
/// owns TTL/Urgency/crypto-header logic) and sent via `reqwest`. We avoid the
/// built-in `IsahcWebPushClient` because (a) it links libcurl, which needs the
/// system CA bundle present on disk — a footgun on slim container images — and
/// (b) the crate's `From<isahc::Error>` impl discards the underlying cause and
/// surfaces every transport failure as `WebPushError::Unspecified`, leaving
/// operators with no signal about what actually broke.
async fn send_webpush(
    subscription_id: i32,
    title: &str,
    body: &str,
    click_url: &str,
    db: &UltrosDb,
    config: &WebPushConfig,
) -> Result<()> {
    use web_push::{
        ContentEncoding, SubscriptionInfo, WebPushError, WebPushMessageBuilder, request_builder,
    };

    let sub = db.get_push_subscription_by_id(subscription_id).await?;
    let endpoint = validate_web_push_endpoint(&sub.endpoint).map_err(anyhow::Error::msg)?;
    let info = SubscriptionInfo::new(endpoint.as_str(), &sub.p256dh, &sub.auth);

    // VAPID signature: parse the operator's private key, attach the `sub`
    // claim with their contact email, then sign.
    let mut sig_builder = vapid_signature_builder(config, &info)?;
    sig_builder.add_claim("sub", config.contact_email.as_str());
    let signature = sig_builder
        .build()
        .map_err(|e| anyhow!("VAPID build failed: {e:?}"))?;

    let payload = build_push_payload(title, body, click_url)?;

    let mut builder = WebPushMessageBuilder::new(&info);
    builder.set_payload(ContentEncoding::Aes128Gcm, &payload);
    builder.set_vapid_signature(signature);
    let message = builder
        .build()
        .map_err(|e| anyhow!("web push build failed: {e:?}"))?;

    let http_req = request_builder::build_request::<reqwest::Body>(message);
    let req = reqwest::Request::try_from(http_req)
        .map_err(|e| anyhow!("push request convert failed: {e}"))?;

    let resp = web_push_http_client()?
        .execute(req)
        .await
        .map_err(|e| anyhow!("push send failed: {e}"))?;

    let status = resp.status();
    let body_bytes = resp
        .bytes()
        .await
        .map_err(|e| anyhow!("push response read failed: {e}"))?;

    match request_builder::parse_response(status, body_bytes.to_vec()) {
        Ok(()) => {
            // Best-effort touch — if the update fails we still report success
            // since the push itself went through.
            let _ = db.touch_push_subscription_last_seen(subscription_id).await;
            Ok(())
        }
        Err(WebPushError::EndpointNotFound(_)) | Err(WebPushError::EndpointNotValid(_)) => {
            let _ = db
                .delete_push_subscription_by_id(sub.user_id, subscription_id)
                .await;
            Err(anyhow!("push subscription expired"))
        }
        Err(e) => Err(anyhow!("push send failed: {e}")),
    }
}

fn vapid_signature_builder<'a>(
    config: &WebPushConfig,
    info: &'a web_push::SubscriptionInfo,
) -> Result<web_push::VapidSignatureBuilder<'a>> {
    use web_push::VapidSignatureBuilder;

    let raw_key = config.private_key_pem.trim();
    let normalized_pem = raw_key.replace("\\n", "\n");
    if normalized_pem.contains("-----BEGIN") {
        VapidSignatureBuilder::from_pem(normalized_pem.as_bytes(), info)
            .map_err(|e| anyhow!("VAPID PEM parse failed: {e:?}"))
    } else {
        VapidSignatureBuilder::from_base64(raw_key, info)
            .map_err(|e| anyhow!("VAPID base64url parse failed: {e:?}"))
    }
}

async fn send_webhook(url: &str, title: &str, body: &str) -> Result<()> {
    // Discord webhook expects JSON with `embeds`. allowed_mentions parse=[] suppresses pings.
    let payload = serde_json::json!({
        "embeds": [{
            "title": title,
            "description": body,
            "color": 0x00c850,
        }],
        "allowed_mentions": { "parse": [] },
    });
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .post(url)
        .json(&payload)
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("webhook returned {status}: {body}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn push_endpoint_accepts_browser_providers_and_opaque_tokens() {
        for endpoint in [
            "https://fcm.googleapis.com/fcm/send/token",
            "https://fcm.googleapis.com/wp/new-format?token=abc%2Fdef",
            "https://updates.push.services.mozilla.com/wpush/v2/token",
            "https://web.push.apple.com/Q/token",
            "https://regional.push.apple.com/future-token-format",
            "https://wns2-par02p.notify.windows.com/w/?token=abc%2Fdef",
            "https://FCM.GOOGLEAPIS.COM:443/fcm/send/token",
        ] {
            assert!(validate_web_push_endpoint(endpoint).is_ok(), "{endpoint}");
        }
    }

    #[test]
    fn push_endpoint_rejects_internal_destinations_and_host_spoofing() {
        // Validation only: none of these URLs is ever resolved or contacted.
        for endpoint in [
            "https://127.0.0.1/push",
            "https://2130706433/push",
            "https://[::1]/push",
            "https://[::ffff:127.0.0.1]/push",
            "https://10.0.0.1/push",
            "https://169.254.169.254/latest/meta-data/",
            "https://localhost/push",
            "https://attacker.example/push",
            "https://fcm.googleapis.com.attacker.example/push",
            "https://attacker.example/fcm.googleapis.com/push",
            "https://evilpush.apple.com/push",
            "https://evilnotify.windows.com/push",
            "https://web.push.apple.com.attacker.example/push",
            "https://notify.windows.com.attacker.example/push",
            "https://attacker.fcm.googleapis.com/push",
            "https://attacker.updates.push.services.mozilla.com/push",
            "https://fcm.googleapis.com@127.0.0.1/push",
        ] {
            assert!(validate_web_push_endpoint(endpoint).is_err(), "{endpoint}");
        }
    }

    #[test]
    fn push_endpoint_rejects_unsafe_url_components() {
        for endpoint in [
            "not a URL",
            "http://fcm.googleapis.com/fcm/send/token",
            "ftp://fcm.googleapis.com/fcm/send/token",
            "https://fcm.googleapis.com:8443/fcm/send/token",
            "https://user@fcm.googleapis.com/fcm/send/token",
            "https://user:password@fcm.googleapis.com/fcm/send/token",
            "https://fcm.googleapis.com/fcm/send/token#fragment",
            "https://fcm.googleapis.com./fcm/send/token",
        ] {
            assert!(validate_web_push_endpoint(endpoint).is_err(), "{endpoint}");
        }
    }

    #[tokio::test]
    async fn push_http_client_does_not_follow_redirects() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // A synthetic local HTTP server exercises the actual delivery client
        // without push credentials or requests to any external service. The
        // production send path separately requires a validated HTTPS endpoint.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let received = socket.read(&mut request).await.unwrap();
            assert!(
                received > 0,
                "the client must send a request before redirecting"
            );
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{address}/redirected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let response = web_push_http_client()
            .unwrap()
            .post(format!("http://{address}/original"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(response.url().path(), "/original");
        server.await.unwrap();
    }

    #[test]
    fn dead_discord_destinations_are_permanent() {
        // The exact pairs behind GlitchTip issues 6877/6878/6879/6885/6889
        // ("Unknown Channel") and 6881/6882/6883/6884 ("Missing Access"),
        // which retried forever and produced ~150 error events a day.
        assert!(is_permanent_discord_failure(404, 10003)); // Unknown Channel
        assert!(is_permanent_discord_failure(403, 50001)); // Missing Access
        assert!(is_permanent_discord_failure(404, 10013)); // Unknown User
        assert!(is_permanent_discord_failure(403, 50007)); // Cannot DM this user
        assert!(is_permanent_discord_failure(403, 50013)); // Missing Permissions
    }

    #[test]
    fn transient_discord_failures_do_not_disable() {
        // Rate limiting is the whole point of retrying.
        assert!(!is_permanent_discord_failure(429, 0));
        // Discord-side outages must never disable a working destination.
        assert!(!is_permanent_discord_failure(500, 0));
        assert!(!is_permanent_discord_failure(503, 0));
        // serenity uses -1 when it couldn't decode the error body — that tells
        // us nothing, so it can't justify disabling anything.
        assert!(!is_permanent_discord_failure(400, -1));
        // An unrecognised 4xx code stays transient rather than guessing.
        assert!(!is_permanent_discord_failure(400, 50035));
    }

    #[test]
    fn a_5xx_never_counts_as_permanent_even_carrying_a_permanent_code() {
        // Defensive: a gateway returning 502 with a stale body shouldn't take
        // out every endpoint at once.
        assert!(!is_permanent_discord_failure(502, 10003));
        assert!(!is_permanent_discord_failure(500, 50001));
    }

    #[test]
    fn non_discord_errors_are_never_permanent() {
        // Webhook/WebPush failures surface as plain anyhow errors with no
        // serenity cause in the chain, so they must not disable the endpoint.
        let err = anyhow!("webhook returned 500: upstream exploded");
        assert!(permanent_failure_reason(&err).is_none());

        let nested = err.context("delivering to endpoint 3");
        assert!(permanent_failure_reason(&nested).is_none());
    }

    #[test]
    fn push_payload_carries_the_callers_click_url() {
        let payload = build_push_payload("Undercut Alert", "body", "/retainers/undercuts").unwrap();
        let decoded: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(decoded["url"], json!("/retainers/undercuts"));
        assert_eq!(decoded["title"], json!("Undercut Alert"));
        assert_eq!(decoded["body"], json!("body"));
    }

    #[test]
    fn parses_discord_dm_from_method_plus_config() {
        let cfg = json!({ "user_id": 1234 });
        let parsed = parse_endpoint_config("DiscordDm", &cfg).unwrap();
        assert_eq!(parsed, EndpointConfig::DiscordDm { user_id: 1234 });
    }

    #[test]
    fn parses_discord_channel_from_method_plus_config() {
        let cfg = json!({ "channel_id": 99 });
        let parsed = parse_endpoint_config("DiscordChannel", &cfg).unwrap();
        assert_eq!(parsed, EndpointConfig::DiscordChannel { channel_id: 99 });
    }

    #[test]
    fn parses_webpush_from_method_plus_config() {
        let cfg = json!({ "subscription_id": 42 });
        let parsed = parse_endpoint_config("WebPush", &cfg).unwrap();
        assert_eq!(
            parsed,
            EndpointConfig::WebPush {
                subscription_id: 42
            }
        );
    }

    #[test]
    fn parses_webhook_from_method_plus_config() {
        let cfg = json!({ "url": "https://discord.com/api/webhooks/1/abc" });
        let parsed = parse_endpoint_config("Webhook", &cfg).unwrap();
        assert_eq!(
            parsed,
            EndpointConfig::Webhook {
                url: "https://discord.com/api/webhooks/1/abc".to_string()
            }
        );
    }

    #[test]
    fn parse_endpoint_ignores_method_field_already_present_in_config() {
        // The splicing overwrites any existing "method" key in the config object —
        // protects against double-tagged rows in the DB.
        let cfg = json!({ "method": "WrongMethod", "user_id": 7 });
        let parsed = parse_endpoint_config("DiscordDm", &cfg).unwrap();
        assert_eq!(parsed, EndpointConfig::DiscordDm { user_id: 7 });
    }

    #[test]
    fn parse_endpoint_rejects_unknown_method() {
        let cfg = json!({ "user_id": 1 });
        assert!(parse_endpoint_config("Pigeon", &cfg).is_err());
    }

    #[test]
    fn parse_endpoint_rejects_missing_required_fields() {
        // DiscordDm requires user_id; missing it is a parse error.
        let cfg = json!({});
        assert!(parse_endpoint_config("DiscordDm", &cfg).is_err());
        // Webhook requires url; missing it is also a parse error.
        assert!(parse_endpoint_config("Webhook", &cfg).is_err());
    }

    #[test]
    fn parse_endpoint_rejects_wrong_type_for_id() {
        let cfg = json!({ "user_id": "not-a-number" });
        assert!(parse_endpoint_config("DiscordDm", &cfg).is_err());
    }

    #[test]
    fn parse_endpoint_treats_non_object_config_as_empty() {
        // If the DB stores null/array/string as config, the splicer turns it into an
        // object with just the method tag, which then fails for missing fields. We
        // only assert that we don't panic and return an error rather than success.
        for bad in [json!(null), json!([]), json!("string"), json!(42)] {
            let r = parse_endpoint_config("DiscordDm", &bad);
            assert!(r.is_err(), "expected err for config: {bad}");
        }
    }
}
