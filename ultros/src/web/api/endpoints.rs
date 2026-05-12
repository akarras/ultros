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
        EndpointMethod::DiscordChannel { channel_id } => (
            "DiscordChannel",
            serde_json::json!({ "channel_id": channel_id }),
        ),
        EndpointMethod::Webhook { url } => ("Webhook", serde_json::json!({ "url": url })),
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
        }),
        "Webhook" => Ok(EndpointMethod::Webhook {
            url: config
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Webhook missing url"))?
                .to_string(),
        }),
        other => Err(anyhow::anyhow!("unknown method {other}")),
    }
}

#[allow(clippy::result_large_err)]
pub(crate) fn validate_endpoint_method(m: &EndpointMethod) -> Result<(), ApiError> {
    match m {
        EndpointMethod::Webhook { url } => validate_discord_webhook_url(url),
        EndpointMethod::DiscordChannel { channel_id } => validate_discord_channel_id(*channel_id),
        EndpointMethod::DiscordDm { .. } => Ok(()),
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
    let method = match req.method {
        EndpointMethod::DiscordDm { user_id: 0 } => EndpointMethod::DiscordDm {
            user_id: user.id as i64,
        },
        other => other,
    };
    validate_endpoint_method(&method)?;
    let (method_str, config) = method_to_db(&method);
    let id = db
        .create_endpoint(user.id as i64, &req.name, method_str, config)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(Endpoint {
        id,
        name: req.name,
        method,
    }))
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
    let Some(serenity_ctx) = crate::alerts::delivery::get_serenity_ctx() else {
        return Ok(Json(ResendResult {
            delivered: false,
            error: Some("Discord client not ready".into()),
        }));
    };
    match crate::alerts::delivery::deliver_to_endpoint(
        &endpoint,
        "Ultros test notification",
        "If you can read this, your endpoint is wired up correctly.",
        &db,
        &serenity_ctx,
    )
    .await
    {
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
    fn method_to_db_round_trip_discord_channel() {
        let m = EndpointMethod::DiscordChannel { channel_id: 12345 };
        let (method, config) = method_to_db(&m);
        assert_eq!(method, "DiscordChannel");
        assert_eq!(config, json!({"channel_id": 12345}));
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
            EndpointMethod::DiscordChannel { channel_id: 2 },
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
        let m = EndpointMethod::DiscordChannel { channel_id: 0 };
        assert!(validate_endpoint_method(&m).is_err());
    }
}
