use super::{PluginApiError, PluginConfig, PluginUser};
use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::sync::Arc;
use ultros_api_types::list::{List, ListPermission, ListWithPermission};
use ultros_api_types::plugin::*;
use ultros_db::UltrosDb;
use ultros_list_doc::{ListDocument, SyncPayload};

pub(super) fn router() -> Router<super::WebState> {
    let lists = Router::new()
        .route("/api/v1/plugin/lists", get(lists))
        .route("/api/v1/plugin/lists/{id}/sync", post(sync_list))
        .layer(DefaultBodyLimit::max(12 * 1024 * 1024));
    Router::new()
        .route("/api/v1/plugin/characters", get(characters))
        .route("/api/v1/plugin/lists/{id}/estimate", get(estimate))
        .route("/api/v1/plugin/characters/observe", post(observe))
        .route("/api/v1/plugin/retainers/register", post(register))
        .route("/api/v1/plugin/characters/{id}/uploads", post(consent))
        .route("/api/v1/plugin/retainers/{id}/listings", post(upload))
        .route("/api/v1/plugin/retainers/snapshots", get(snapshots))
        .route("/api/v1/plugin/notifications", get(notifications))
        .route(
            "/api/v1/plugin/notifications/stream",
            get(super::notifications::stream),
        )
        .route("/api/v1/plugin/notifications/ack", post(acknowledge))
        .route("/api/v1/plugin/notifications/read", post(mark_read))
        .route("/api/v1/plugin/notifications/test", post(test_notification))
        .layer(DefaultBodyLimit::max(32 * 1024))
        .merge(lists)
}

async fn estimate(
    State(state): State<super::WebState>,
    PluginUser(user): PluginUser,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, PluginApiError> {
    use ultros_calc::list_estimate::{LineRequest, estimate_cart};
    use ultros_db::plugin::PluginError as E;
    let stored = state
        .db
        .plugin_list_snapshot(&user, id)
        .await
        .map_err(list_error)?;
    let doc = ListDocument::from_snapshot(&stored.snapshot).map_err(|_| E::Storage)?;
    let scope = doc.meta().scope.ok_or(E::Invalid("list_scope_required"))?;
    let selector = match scope {
        ultros_api_types::world_helper::AnySelector::World(v) => {
            ultros_db::world_data::world_cache::AnySelector::World(v)
        }
        ultros_api_types::world_helper::AnySelector::Datacenter(v) => {
            ultros_db::world_data::world_cache::AnySelector::Datacenter(v)
        }
        ultros_api_types::world_helper::AnySelector::Region(v) => {
            ultros_db::world_data::world_cache::AnySelector::Region(v)
        }
    };
    let resolved = state
        .world_cache
        .lookup_selector(&selector)
        .map_err(|_| E::Invalid("invalid_list_scope"))?;
    let worlds = state
        .world_cache
        .get_all_worlds_in(&resolved)
        .ok_or(E::Invalid("invalid_list_scope"))?;
    let rows = doc.rows();
    if rows.len() > 1000 {
        return Err(E::Invalid("list_too_large_for_estimate").into());
    }
    let ids = rows.iter().map(|r| r.key.item_id).collect::<Vec<_>>();
    let raw = state
        .db
        .get_listings_for_items_in_worlds(&worlds, &ids)
        .await
        .map_err(|_| E::Storage)?;
    let listings = raw
        .into_iter()
        .map(ultros_api_types::ActiveListing::from)
        .collect::<Vec<_>>();
    let requests = rows
        .iter()
        .enumerate()
        .map(|(index, row)| LineRequest {
            row_id: index as i32,
            item_id: row.key.item_id,
            hq: row.key.quality.into(),
            requested: row.need.clamp(0, i32::MAX as i64) as i32,
            acquired: row.acquired.clamp(0, i32::MAX as i64) as i32,
        })
        .collect::<Vec<_>>();
    let result = estimate_cart(requests.into_iter().map(|r| (r, listings.as_slice())));
    Ok(Json(
        serde_json::json!({"version":STANDARD.encode(doc.version()),"scope":resolved.get_name(),"observed_at":chrono::Utc::now(),"total":result.total,"incomplete":result.is_incomplete(),"unpriced_units":result.unpriced_units,"lines":result.lines.into_iter().map(|r|serde_json::json!({"item_id":r.item_id,"hq":r.hq,"remaining":r.remaining,"priced_units":r.priced_units,"total":r.total,"status":format!("{:?}",r.status)})).collect::<Vec<_>>()}),
    ))
}
async fn characters(
    State(db): State<UltrosDb>,
    PluginUser(user): PluginUser,
) -> Result<Json<Vec<UnverifiedCharacter>>, PluginApiError> {
    Ok(Json(db.plugin_characters(&user).await?))
}
async fn observe(
    State(db): State<UltrosDb>,
    Extension(config): Extension<Arc<PluginConfig>>,
    PluginUser(user): PluginUser,
    Json(request): Json<ObserveCharacter>,
) -> Result<Json<Vec<UnverifiedCharacter>>, PluginApiError> {
    Ok(Json(
        db.observe_plugin_character(&user, &config.keys, &request)
            .await?,
    ))
}
async fn register(
    State(db): State<UltrosDb>,
    Extension(config): Extension<Arc<PluginConfig>>,
    PluginUser(user): PluginUser,
    Json(request): Json<RegisterRetainers>,
) -> Result<Json<Vec<UnverifiedCharacter>>, PluginApiError> {
    Ok(Json(
        db.register_plugin_retainers(&user, &config.keys, &request)
            .await?,
    ))
}
async fn consent(
    State(db): State<UltrosDb>,
    PluginUser(user): PluginUser,
    Path(id): Path<i64>,
    Json(request): Json<UploadConsent>,
) -> Result<Json<Vec<UnverifiedCharacter>>, PluginApiError> {
    Ok(Json(
        db.set_plugin_upload_consent(&user, id, &request).await?,
    ))
}
async fn upload(
    State(db): State<UltrosDb>,
    PluginUser(user): PluginUser,
    Path(id): Path<i64>,
    Json(request): Json<ListingCapture>,
) -> Result<Json<CaptureReceipt>, PluginApiError> {
    Ok(Json(db.upload_plugin_listings(&user, id, &request).await?))
}
async fn snapshots(
    State(db): State<UltrosDb>,
    PluginUser(user): PluginUser,
) -> Result<Json<Vec<RetainerSnapshot>>, PluginApiError> {
    Ok(Json(db.plugin_retainer_snapshots(&user).await?))
}
async fn notifications(
    State(db): State<UltrosDb>,
    PluginUser(user): PluginUser,
) -> Result<Json<DeliveryBatch>, PluginApiError> {
    Ok(Json(db.pending_plugin_deliveries(&user).await?))
}
async fn acknowledge(
    State(db): State<UltrosDb>,
    PluginUser(user): PluginUser,
    Json(ids): Json<DeliveryIds>,
) -> Result<axum::http::StatusCode, PluginApiError> {
    db.acknowledge_plugin_deliveries(&user, &ids, false).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
async fn mark_read(
    State(db): State<UltrosDb>,
    PluginUser(user): PluginUser,
    Json(ids): Json<DeliveryIds>,
) -> Result<axum::http::StatusCode, PluginApiError> {
    db.acknowledge_plugin_deliveries(&user, &ids, true).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
async fn test_notification(
    State(db): State<UltrosDb>,
    PluginUser(user): PluginUser,
    Json(request): Json<OperationRequest>,
) -> Result<axum::http::StatusCode, PluginApiError> {
    db.test_plugin_delivery(&user, &request.operation_id)
        .await?;
    super::wake_notifications(
        user.session
            .account_id
            .parse()
            .map_err(|_| ultros_db::plugin::PluginError::Unauthorized)?,
    );
    Ok(axum::http::StatusCode::ACCEPTED)
}

fn list_error(error: ultros_db::list_doc::ListDocError) -> PluginApiError {
    use ultros_db::plugin::PluginError as P;
    use ultros_db::{list_doc::ListDocError as E, lists::ListError};
    PluginApiError(match error {
        E::Plugin(e) => e,
        E::List(ListError::NotFound) => P::Conflict("list_not_found"),
        E::List(ListError::Forbidden(_)) => P::Conflict("list_forbidden"),
        E::MetaForbidden => P::Conflict("metadata_forbidden"),
        E::MissingHistory => P::Conflict("missing_history"),
        E::InvalidUpdate | E::Doc(_) => P::Invalid("invalid_document_update"),
        _ => P::Storage,
    })
}
async fn lists(
    State(db): State<UltrosDb>,
    PluginUser(user): PluginUser,
) -> Result<Json<Vec<ListWithPermission>>, PluginApiError> {
    let session = db.plugin_session(&user).await?;
    if !session.scopes.iter().any(|s| s == "lists:read") {
        return Err(ultros_db::plugin::PluginError::Forbidden.into());
    }
    let mut result = Vec::new();
    for (list, owner_name) in db
        .get_lists_for_user(
            session
                .account_id
                .parse()
                .map_err(|_| ultros_db::plugin::PluginError::Unauthorized)?,
        )
        .await
        .map_err(|_| ultros_db::plugin::PluginError::Storage)?
    {
        let mut permission = db
            .get_permission(
                list.id,
                session
                    .account_id
                    .parse()
                    .map_err(|_| ultros_db::plugin::PluginError::Unauthorized)?,
            )
            .await
            .map_err(|_| ultros_db::plugin::PluginError::Storage)?
            .min(ListPermission::Write);
        if !session.scopes.iter().any(|s| s == "lists:write") {
            permission = permission.min(ListPermission::Read);
        }
        if permission >= ListPermission::Read {
            result.push(ListWithPermission {
                list: List::try_from(list).map_err(|_| ultros_db::plugin::PluginError::Storage)?,
                permission,
                owner_name,
            });
        }
    }
    Ok(Json(result))
}
async fn sync_list(
    State(state): State<super::WebState>,
    PluginUser(user): PluginUser,
    Path(id): Path<i32>,
    Json(request): Json<SyncList>,
) -> Result<Json<SyncedList>, PluginApiError> {
    if request.version.len() > 65536 {
        return Err(ultros_db::plugin::PluginError::Invalid("invalid_version").into());
    }
    let version = STANDARD
        .decode(request.version)
        .map_err(|_| ultros_db::plugin::PluginError::Invalid("invalid_version"))?;
    if let Some(update) = request.update {
        if update.len() > 8 * 1024 * 1024 / 3 * 4 + 8 {
            return Err(ultros_db::plugin::PluginError::Invalid("update_too_large").into());
        }
        let update = STANDARD
            .decode(update)
            .map_err(|_| ultros_db::plugin::PluginError::Invalid("invalid_update"))?;
        state
            .list_sync
            .apply_plugin_update(id, &user, &update)
            .await
            .map_err(list_error)?;
    }
    let stored = state
        .db
        .plugin_list_snapshot(&user, id)
        .await
        .map_err(list_error)?;
    let document = ListDocument::from_snapshot(&stored.snapshot)
        .map_err(|_| ultros_db::plugin::PluginError::Storage)?;
    let (kind, payload) = match document
        .sync_payload(&version)
        .map_err(|_| ultros_db::plugin::PluginError::Invalid("invalid_version"))?
    {
        SyncPayload::Snapshot(p) => ("snapshot", p),
        SyncPayload::Updates(p) => ("updates", p),
        SyncPayload::UpToDate => ("up_to_date", Vec::new()),
    };
    Ok(Json(SyncedList {
        version: STANDARD.encode(document.version()),
        kind: kind.into(),
        payload: STANDARD.encode(payload),
    }))
}
