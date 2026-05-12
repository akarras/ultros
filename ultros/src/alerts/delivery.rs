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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "method")]
pub(crate) enum EndpointConfig {
    #[serde(rename = "DiscordChannel")]
    DiscordChannel { channel_id: i64 },
    #[serde(rename = "DiscordDm")]
    DiscordDm { user_id: i64 },
    #[serde(rename = "Webhook")]
    Webhook { url: String },
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
pub(crate) async fn deliver_to_endpoint(
    endpoint: &ultros_db::entity::notification_endpoint::Model,
    title: &str,
    body: &str,
    _db: &UltrosDb,
    ctx: &serenity_prelude::Context,
) -> Result<()> {
    let parsed = parse_endpoint_config(&endpoint.method, &endpoint.config)?;
    match parsed {
        EndpointConfig::DiscordChannel { channel_id } => {
            send_to_channel(channel_id, title, body, ctx).await
        }
        EndpointConfig::DiscordDm { user_id } => send_dm(user_id, title, body, ctx).await,
        EndpointConfig::Webhook { url } => send_webhook(&url, title, body).await,
    }
}

/// Look up all notification endpoints for an alert and dispatch the message via each.
/// Returns Ok(()) if at least one delivered; Err describing the last failure otherwise.
pub(crate) async fn dispatch_alert(
    alert_id: i32,
    title: &str,
    body: &str,
    db: &UltrosDb,
    ctx: &serenity_prelude::Context,
) -> Result<()> {
    let endpoints = db.get_notification_endpoints_for_alert(alert_id).await?;

    if endpoints.is_empty() {
        return Err(anyhow!("alert {alert_id} has no notification rules"));
    }

    let mut last_err: Option<anyhow::Error> = None;
    let mut any_ok = false;

    for endpoint in endpoints {
        match deliver_to_endpoint(&endpoint, title, body, db, ctx).await {
            Ok(()) => any_ok = true,
            Err(e) => {
                error!("delivery failed for alert {alert_id}: {e}");
                last_err = Some(e);
            }
        }
    }

    if any_ok {
        Ok(())
    } else {
        Err(last_err.unwrap_or_else(|| anyhow!("no deliveries succeeded")))
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
    let resp = reqwest::Client::new()
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
