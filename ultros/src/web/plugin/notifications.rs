//! SSE carries wake-ups only. The scoped durable HTTP queue remains authoritative.
use super::{PluginApiError, PluginUser, WebState};
use axum::{
    extract::State,
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use futures::StreamExt;
use std::{convert::Infallible, sync::OnceLock, time::Duration};
use ultros_db::plugin::{PluginError, PluginPrincipal};

fn wake_bus() -> &'static tokio::sync::broadcast::Sender<i64> {
    static BUS: OnceLock<tokio::sync::broadcast::Sender<i64>> = OnceLock::new();
    BUS.get_or_init(|| tokio::sync::broadcast::channel(256).0)
}
pub(super) fn wake(owner: i64) {
    let _ = wake_bus().send(owner);
}

struct Feed {
    db: ultros_db::UltrosDb,
    user: PluginPrincipal,
    owner: i64,
    events: crate::event::EventBus<crate::event::NotificationEvent>,
    wakes: tokio::sync::broadcast::Receiver<i64>,
    check: tokio::time::Interval,
    deadline: tokio::time::Instant,
    shutdown: tokio_util::sync::CancellationToken,
}

pub(super) async fn stream(
    State(state): State<WebState>,
    PluginUser(user): PluginUser,
) -> Result<Response, PluginApiError> {
    let session = state.db.plugin_session(&user).await?;
    if !session
        .scopes
        .iter()
        .any(|scope| scope == "notifications:read")
    {
        return Err(PluginError::Forbidden.into());
    }
    state
        .db
        .plugin_rate_limit(&format!("notification-stream:{}", session.id), 20, 60)
        .await?;
    let feed = Feed {
        db: state.db,
        owner: session
            .account_id
            .parse()
            .map_err(|_| PluginError::Unauthorized)?,
        user,
        events: state.event_receivers.notifications.resubscribe(),
        wakes: wake_bus().subscribe(),
        check: tokio::time::interval(Duration::from_secs(60)),
        deadline: tokio::time::Instant::now() + Duration::from_secs(120),
        shutdown: state.token,
    };
    let events = futures::stream::unfold(feed, |mut feed| async move {
        loop {
            let wake = tokio::select! {
                _=feed.shutdown.cancelled()=>return None,
                _=tokio::time::sleep_until(feed.deadline)=>return None,
                _=feed.check.tick()=>true,
                owner=feed.wakes.recv()=>match owner {
                    Ok(owner)=>owner==feed.owner,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>true,
                    Err(tokio::sync::broadcast::error::RecvError::Closed)=>return None,
                },
                event=feed.events.recv()=>match event {
                    Ok(event)=>event.as_ref().owner==feed.owner,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>true,
                    Err(tokio::sync::broadcast::error::RecvError::Closed)=>return None,
                }
            };
            if !wake {
                continue;
            }
            // Re-check the bearer grant, including expiry/revocation, before
            // each wake-up. No event text or credentials enter this stream.
            let Ok(session) = feed.db.plugin_session(&feed.user).await else {
                return None;
            };
            if !session
                .scopes
                .iter()
                .any(|scope| scope == "notifications:read")
            {
                return None;
            }
            return Some((
                Ok::<_, Infallible>(Event::default().event("notifications").data("ready")),
                feed,
            ));
        }
    });
    // Keep-alive/body consumers may poll once more after expiry or revocation.
    // Unfold itself panics on a repeated EOF poll.
    let mut response = Sse::new(events.fuse())
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response();
    response
        .headers_mut()
        .insert("x-accel-buffering", "no".parse().expect("constant header"));
    Ok(response)
}
