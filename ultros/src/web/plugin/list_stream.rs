//! Authenticated, list-scoped wake-ups. Durable HTTP sync remains authoritative.
use super::{PluginApiError, PluginUser, WebState};
use axum::{
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
};
use std::time::Duration;
use ultros_api_types::websocket::ListEventData;
use ultros_db::plugin::PluginPrincipal;

pub(super) async fn stream(
    ws: WebSocketUpgrade,
    State(state): State<WebState>,
    PluginUser(user): PluginUser,
    Path(id): Path<i32>,
) -> Result<Response, PluginApiError> {
    state
        .db
        .plugin_list_readable(&user, id)
        .await
        .map_err(super::flows::list_error)?;
    state
        .db
        .plugin_rate_limit(&format!("list-stream:{}", user.session.id), 30, 60)
        .await?;
    // Subscribe before the initial wake, so writes during the handshake are
    // either in that HTTP sync or still waiting on this receiver.
    let docs = state.event_receivers.list_docs.resubscribe();
    let lists = state.event_receivers.lists.resubscribe();
    Ok(ws
        .max_message_size(512)
        .max_frame_size(512)
        .on_upgrade(move |socket| listen(socket, state, user, id, docs, lists)))
}

async fn listen(
    mut socket: WebSocket,
    state: WebState,
    user: PluginPrincipal,
    id: i32,
    mut docs: crate::event::EventBus<crate::event::ListDocEvent>,
    mut lists: crate::event::EventBus<ListEventData>,
) {
    if state.db.plugin_list_readable(&user, id).await.is_err() {
        return;
    }
    let message = serde_json::json!({"event":"changed","list_id":id}).to_string();
    if !send(
        &mut socket,
        Message::Text(message.clone().into()),
        &state.token,
    )
    .await
    {
        return;
    }
    let mut dirty = false;
    let mut flush = tokio::time::interval(Duration::from_millis(150));
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut check = tokio::time::interval(Duration::from_secs(30));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    loop {
        tokio::select! {
            _=state.token.cancelled()=>break,
            _=tokio::time::sleep_until(deadline)=>break,
            event=docs.recv()=>match event {
                Ok(event)=>dirty|=event.as_ref().list_id==id,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>dirty=true,
                Err(tokio::sync::broadcast::error::RecvError::Closed)=>break,
            },
            event=lists.recv()=>match event {
                Ok(event)=>dirty|=matches!(event.as_ref().as_ref(),ListEventData::List(list) if list.id==id),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>dirty=true,
                Err(tokio::sync::broadcast::error::RecvError::Closed)=>break,
            },
            _=flush.tick(),if dirty=>{
                // Membership and the bearer grant are checked before every
                // notification. A stale connection is never an access proof.
                if state.db.plugin_list_readable(&user,id).await.is_err(){break;}
                if !send(&mut socket,Message::Text(message.clone().into()),&state.token).await{return;}
                dirty=false;
            },
            _=check.tick()=>{
                if state.db.plugin_list_readable(&user,id).await.is_err(){break;}
                if !send(&mut socket,Message::Ping(Vec::new().into()),&state.token).await{return;}
            },
            incoming=socket.recv()=>match incoming {
                Some(Ok(Message::Ping(_) | Message::Pong(_)))=>{},
                // This endpoint is receive-only; client document edits use
                // the existing bounded, authenticated HTTP sync endpoint.
                _=>break,
            }
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Close(None))).await;
}

async fn send(
    socket: &mut WebSocket,
    message: Message,
    shutdown: &tokio_util::sync::CancellationToken,
) -> bool {
    tokio::select! {
        _=shutdown.cancelled()=>false,
        result=tokio::time::timeout(Duration::from_secs(5),socket.send(message))=>matches!(result,Ok(Ok(()))),
    }
}
