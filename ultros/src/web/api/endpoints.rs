use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::Value as JsonValue;
use ultros_api_types::alert::{
    CreateEndpointRequest, Endpoint, EndpointMethod, ResendResult, UpdateEndpointRequest,
};
use ultros_db::UltrosDb;

use crate::web::api::endpoint_validation::{
    validate_discord_channel_id, validate_discord_webhook_url,
};
use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;

pub(crate) fn method_to_db(m: &EndpointMethod) -> (&'static str, JsonValue) {
    match m {
        EndpointMethod::DiscordDm { user_id } => {
            ("DiscordDm", serde_json::json!({ "user_id": user_id }))
        }
        EndpointMethod::DiscordChannel {
            channel_id,
            channel_name,
            guild_id,
            guild_name,
        } => {
            // Persist whichever resolved fields we have. Older rows pre-resolution will
            // simply be missing them; readers (db_to_method, delivery.rs) tolerate that.
            let mut obj = serde_json::Map::new();
            obj.insert("channel_id".into(), serde_json::json!(channel_id));
            if let Some(name) = channel_name {
                obj.insert("channel_name".into(), serde_json::json!(name));
            }
            if let Some(gid) = guild_id {
                obj.insert("guild_id".into(), serde_json::json!(gid));
            }
            if let Some(gname) = guild_name {
                obj.insert("guild_name".into(), serde_json::json!(gname));
            }
            ("DiscordChannel", serde_json::Value::Object(obj))
        }
        EndpointMethod::Webhook { url } => ("Webhook", serde_json::json!({ "url": url })),
        EndpointMethod::WebPush { subscription_id } => (
            "WebPush",
            serde_json::json!({ "subscription_id": subscription_id }),
        ),
    }
}

pub(crate) fn db_to_method(method: &str, config: &JsonValue) -> anyhow::Result<EndpointMethod> {
    match method {
        "DiscordDm" => Ok(EndpointMethod::DiscordDm {
            user_id: config
                .get("user_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow::anyhow!("DiscordDm missing user_id"))?,
        }),
        "DiscordChannel" => Ok(EndpointMethod::DiscordChannel {
            channel_id: config
                .get("channel_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow::anyhow!("DiscordChannel missing channel_id"))?,
            channel_name: config
                .get("channel_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            guild_id: config.get("guild_id").and_then(|v| v.as_i64()),
            guild_name: config
                .get("guild_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        }),
        "Webhook" => Ok(EndpointMethod::Webhook {
            url: config
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Webhook missing url"))?
                .to_string(),
        }),
        "WebPush" => Ok(EndpointMethod::WebPush {
            subscription_id: config
                .get("subscription_id")
                .and_then(|v| v.as_i64())
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| anyhow::anyhow!("WebPush missing subscription_id"))?,
        }),
        other => Err(anyhow::anyhow!("unknown method {other}")),
    }
}

#[allow(clippy::result_large_err)]
pub(crate) fn validate_endpoint_method(m: &EndpointMethod) -> Result<(), ApiError> {
    match m {
        EndpointMethod::Webhook { url } => validate_discord_webhook_url(url),
        EndpointMethod::DiscordChannel { channel_id, .. } => {
            validate_discord_channel_id(*channel_id)
        }
        EndpointMethod::DiscordDm { .. } => Ok(()),
        EndpointMethod::WebPush { subscription_id } => {
            // WebPush endpoints are created via POST /api/v1/push/subscribe, never
            // through the generic CRUD — the row is meaningless without a real
            // push_subscription on the other side. The id check is belt-and-braces:
            // even if a caller squeezes past the create route restriction, an
            // obviously-bogus id (0, negative) is rejected here.
            if *subscription_id <= 0 {
                return Err(ApiError::AnyhowError(anyhow::anyhow!(
                    "invalid WebPush subscription_id"
                )));
            }
            Err(ApiError::AnyhowError(anyhow::anyhow!(
                "WebPush endpoints must be created via /api/v1/push/subscribe"
            )))
        }
    }
}

pub(crate) async fn list_endpoints(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<Endpoint>>, ApiError> {
    let rows = db
        .list_endpoints(user.id as i64)
        .await
        .map_err(ApiError::from)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let method = db_to_method(&r.method, &r.config).map_err(ApiError::from)?;
        out.push(Endpoint {
            id: r.id,
            name: r.name,
            method,
        });
    }
    Ok(Json(out))
}

pub(crate) async fn create_endpoint(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(req): Json<CreateEndpointRequest>,
) -> Result<Json<Endpoint>, ApiError> {
    // Frontend hack: DiscordDm with user_id=0 means "use the authenticated user's id".
    let mut method = match req.method {
        EndpointMethod::DiscordDm { user_id: 0 } => EndpointMethod::DiscordDm {
            user_id: user.id as i64,
        },
        other => other,
    };
    validate_endpoint_method(&method)?;

    // The display name we will store. Defaults to whatever the client sent; for a
    // freshly resolved DiscordChannel we replace it with the real channel name so
    // operators don't have to type "#general" manually.
    let mut name = req.name.clone();

    // For DiscordChannel endpoints we require the live serenity context, resolve
    // the channel/guild, and verify the caller has admin in that guild before
    // accepting the endpoint. This is the only way to make sure a malicious user
    // can't spam someone else's server by typing in their channel id.
    if let EndpointMethod::DiscordChannel { channel_id, .. } = method {
        let ctx = crate::alerts::delivery::get_serenity_ctx().ok_or_else(|| {
            ApiError::from(anyhow::anyhow!(
                "Discord bot is not connected; cannot validate channel right now"
            ))
        })?;
        let resolved = crate::web::api::discord_lookup::resolve_channel(&ctx, channel_id).await?;
        crate::web::api::discord_lookup::require_user_is_guild_admin(
            &ctx,
            resolved.guild_id,
            user.id as i64,
        )
        .await?;
        // If the caller didn't pick a name (or sent something obviously stub-like
        // — empty / "Channel <id>"), use the resolved name.
        let trimmed = name.trim();
        let stub = format!("Channel {channel_id}");
        if trimmed.is_empty() || trimmed == stub {
            name = format!("#{} ({})", resolved.channel_name, resolved.guild_name);
        }
        method = EndpointMethod::DiscordChannel {
            channel_id,
            channel_name: Some(resolved.channel_name),
            guild_id: Some(resolved.guild_id),
            guild_name: Some(resolved.guild_name),
        };
    }

    let (method_str, config) = method_to_db(&method);
    let id = db
        .create_endpoint(user.id as i64, &name, method_str, config)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(Endpoint { id, name, method }))
}

pub(crate) async fn update_endpoint(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
    Json(req): Json<UpdateEndpointRequest>,
) -> Result<Json<()>, ApiError> {
    let method_and_config = match &req.method {
        Some(m) => {
            validate_endpoint_method(m)?;
            let (method, config) = method_to_db(m);
            Some((method.to_string(), config))
        }
        None => None,
    };
    db.update_endpoint(user.id as i64, id, req.name, method_and_config)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

pub(crate) async fn delete_endpoint(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
) -> Result<Json<()>, ApiError> {
    db.delete_endpoint(user.id as i64, id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

pub(crate) async fn test_endpoint(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
) -> Result<Json<ResendResult>, ApiError> {
    let endpoint = db
        .get_endpoint_owned_by(user.id as i64, id)
        .await
        .map_err(ApiError::from)?;

    // Only Discord-routed endpoints need the bot context. Webhook + WebPush use
    // their own HTTP clients and were previously failing this branch even though
    // serenity isn't on their delivery path. Resolving the ctx is a function of
    // method, not of the endpoint id.
    let needs_ctx = matches!(endpoint.method.as_str(), "DiscordDm" | "DiscordChannel");
    let serenity_ctx = if needs_ctx {
        match crate::alerts::delivery::get_serenity_ctx() {
            Some(ctx) => Some(ctx),
            None => {
                return Ok(Json(ResendResult {
                    delivered: false,
                    error: Some(
                        "Discord bot isn't connected on this deployment, so DM/channel \
                         endpoints can't be tested. Webhook and browser-push endpoints \
                         still work — try one of those, or check the server logs to see \
                         why the bot didn't start."
                            .into(),
                    ),
                }));
            }
        }
    } else {
        // The serenity-aware deliver_to_endpoint takes a `&Context` it never reads
        // for non-Discord methods; we still need a value to pass through. Borrow
        // from a possibly-set ctx if there is one (cheap), otherwise we have to
        // hand-roll the dispatch for non-Discord methods.
        crate::alerts::delivery::get_serenity_ctx()
    };

    let result = if let Some(ctx) = serenity_ctx.as_ref() {
        crate::alerts::delivery::deliver_to_endpoint(
            &endpoint,
            "Ultros test notification",
            "If you can read this, your endpoint is wired up correctly.",
            &db,
            ctx,
        )
        .await
    } else {
        // Webhook/WebPush only — dispatch directly so we don't need a live ctx.
        crate::alerts::delivery::deliver_non_discord_endpoint(
            &endpoint,
            "Ultros test notification",
            "If you can read this, your endpoint is wired up correctly.",
            &db,
        )
        .await
    };

    match result {
        Ok(()) => Ok(Json(ResendResult {
            delivered: true,
            error: None,
        })),
        Err(e) => Ok(Json(ResendResult {
            delivered: false,
            error: Some(format!("{e}")),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use ultros_api_types::alert::EndpointMethod;

    #[test]
    fn method_to_db_round_trip_discord_dm() {
        let m = EndpointMethod::DiscordDm { user_id: 99 };
        let (method, config) = method_to_db(&m);
        assert_eq!(method, "DiscordDm");
        assert_eq!(config, json!({"user_id": 99}));
    }

    #[test]
    fn method_to_db_round_trip_discord_channel_unresolved() {
        let m = EndpointMethod::DiscordChannel {
            channel_id: 12345,
            channel_name: None,
            guild_id: None,
            guild_name: None,
        };
        let (method, config) = method_to_db(&m);
        assert_eq!(method, "DiscordChannel");
        assert_eq!(config, json!({"channel_id": 12345}));
    }

    #[test]
    fn method_to_db_round_trip_discord_channel_resolved() {
        let m = EndpointMethod::DiscordChannel {
            channel_id: 12345,
            channel_name: Some("general".into()),
            guild_id: Some(999),
            guild_name: Some("My FC".into()),
        };
        let (method, config) = method_to_db(&m);
        assert_eq!(method, "DiscordChannel");
        assert_eq!(
            config,
            json!({
                "channel_id": 12345,
                "channel_name": "general",
                "guild_id": 999,
                "guild_name": "My FC",
            })
        );
    }

    #[test]
    fn db_to_method_recovers_optional_channel_fields_when_present() {
        let cfg = json!({
            "channel_id": 7,
            "channel_name": "logs",
            "guild_id": 42,
            "guild_name": "Test",
        });
        let m = db_to_method("DiscordChannel", &cfg).unwrap();
        assert_eq!(
            m,
            EndpointMethod::DiscordChannel {
                channel_id: 7,
                channel_name: Some("logs".into()),
                guild_id: Some(42),
                guild_name: Some("Test".into()),
            }
        );
    }

    #[test]
    fn db_to_method_tolerates_legacy_channel_rows_without_resolved_fields() {
        // Rows created before the channel-name resolution feature only carry
        // channel_id; the new optional fields default to None.
        let cfg = json!({ "channel_id": 7 });
        let m = db_to_method("DiscordChannel", &cfg).unwrap();
        assert_eq!(
            m,
            EndpointMethod::DiscordChannel {
                channel_id: 7,
                channel_name: None,
                guild_id: None,
                guild_name: None,
            }
        );
    }

    #[test]
    fn method_to_db_round_trip_webhook() {
        let url = "https://discord.com/api/webhooks/1/abc";
        let m = EndpointMethod::Webhook { url: url.into() };
        let (method, config) = method_to_db(&m);
        assert_eq!(method, "Webhook");
        assert_eq!(config, json!({"url": url}));
    }

    #[test]
    fn db_to_method_round_trip_all_three() {
        for m in [
            EndpointMethod::DiscordDm { user_id: 1 },
            EndpointMethod::DiscordChannel {
                channel_id: 2,
                channel_name: None,
                guild_id: None,
                guild_name: None,
            },
            EndpointMethod::Webhook {
                url: "https://discord.com/api/webhooks/1/abc".into(),
            },
        ] {
            let (method, config) = method_to_db(&m);
            let back = db_to_method(method, &config).unwrap();
            assert_eq!(m, back);
        }
    }

    #[test]
    fn validate_method_rejects_bad_webhook_url() {
        let m = EndpointMethod::Webhook {
            url: "http://evil.example/api/webhooks/1/x".into(),
        };
        assert!(validate_endpoint_method(&m).is_err());
    }

    #[test]
    fn validate_method_rejects_zero_channel_id() {
        let m = EndpointMethod::DiscordChannel {
            channel_id: 0,
            channel_name: None,
            guild_id: None,
            guild_name: None,
        };
        assert!(validate_endpoint_method(&m).is_err());
    }
}
