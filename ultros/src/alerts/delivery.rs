use anyhow::{Result, anyhow};
use poise::serenity_prelude::{
    self, Color, CreateAllowedMentions, CreateEmbed, CreateMessage, UserId,
};
use serde::Deserialize;
use tracing::error;
use ultros_db::UltrosDb;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "method")]
pub(crate) enum EndpointConfig {
    #[serde(rename = "DiscordChannel")]
    DiscordChannel { channel_id: i64 },
    #[serde(rename = "DiscordDm")]
    DiscordDm { user_id: i64 },
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
        // Re-construct the tagged enum from the method string + config JSON object.
        let mut config_obj = serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
            endpoint.config.clone(),
        )
        .unwrap_or_default();
        config_obj.insert(
            "method".to_string(),
            serde_json::Value::String(endpoint.method.clone()),
        );
        let parsed: EndpointConfig =
            match serde_json::from_value(serde_json::Value::Object(config_obj)) {
                Ok(p) => p,
                Err(e) => {
                    last_err = Some(anyhow!("bad endpoint config for {}: {e}", endpoint.id));
                    continue;
                }
            };

        let result = match parsed {
            EndpointConfig::DiscordChannel { channel_id } => {
                send_to_channel(channel_id, title, body, ctx).await
            }
            EndpointConfig::DiscordDm { user_id } => send_dm(user_id, title, body, ctx).await,
        };
        match result {
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
            CreateMessage::new().embed(
                CreateEmbed::new()
                    .color(Color::from_rgb(0, 200, 80))
                    .title(title)
                    .description(body),
            ),
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
