use std::{
    collections::HashSet,
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
};

use futures::{
    SinkExt, StreamExt,
    future::{Either, select},
    stream::{BoxStream, SelectAll},
};

use tokio_stream::wrappers::BroadcastStream;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use ultros_api_types::websocket::{
    ClientMessage, FilterPredicate, ListEventData, ListingEventData, SaleEventData, ServerClient,
    SocketMessageType,
};
use ultros_api_types::{websocket::EventType as WEvent, world_helper::WorldHelper};

use crate::event::{EventReceivers, EventType, ListDocEvent};
use crate::lists::{Actor, ListSync, Origin};
use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;
use crate::web::shutdown::until_shutdown;
use ultros_api_types::list::ListPermission;
use ultros_db::UltrosDb;
use ultros_db::list_doc::ListDocError;

const MAX_SUBSCRIPTIONS_PER_SOCKET: usize = 64;

/// Distinguishes sockets so a relay never echoes an update to its sender.
static NEXT_SOCKET_ID: AtomicU64 = AtomicU64::new(1);

/// Which relay-stream subscribers should forward this document update: every
/// subscriber of the same list except the socket that sent it (spec section 5).
fn relay_for(event: &ListDocEvent, list_id: i32, socket_id: u64) -> Option<Vec<u8>> {
    if event.list_id != list_id || event.origin_socket == Some(socket_id) {
        return None;
    }
    Some(event.update.clone())
}

pub(crate) async fn real_time_data(
    ws: WebSocketUpgrade,
    user: Result<AuthDiscordUser, ApiError>,
    State(events): State<EventReceivers>,
    State(worlds): State<Arc<WorldHelper>>,
    State(db): State<UltrosDb>,
    State(token): State<CancellationToken>,
    State(list_sync): State<ListSync>,
) -> Response {
    let user = user.ok();
    info!("Handling websocket");
    ws.on_upgrade(move |websocket| async move {
        info!("Upgrading websocket");
        if let Err(e) = handle_socket(websocket, events, worlds, db, user, token, list_sync).await {
            error!("{e:?}");
        }
    })
}

impl<T> From<EventType<T>> for ultros_api_types::websocket::EventType<T> {
    fn from(value: EventType<T>) -> Self {
        match value {
            EventType::Remove(t) => WEvent::Removed(t),
            EventType::Add(t) => WEvent::Added(t),
            EventType::Update(t) => WEvent::Updated(t),
        }
    }
}

fn janky_map_event_type<T, Y>(e: EventType<T>, data: Y) -> WEvent<Y> {
    match e {
        EventType::Remove(_) => WEvent::Removed(data),
        EventType::Add(_) => WEvent::Added(data),
        EventType::Update(_) => WEvent::Updated(data),
    }
}

fn process_listings(
    l: Option<EventType<Arc<ListingEventData>>>,
    filter: &FilterPredicate,
    worlds: &WorldHelper,
) -> Option<ServerClient> {
    let l = l?;
    let world_id = l.as_ref().world_id;
    let item_id = l.as_ref().item_id;
    let listings: Vec<_> = l
        .as_ref()
        .listings
        .iter()
        .filter(|data| filter.filter(worlds, *data))
        .cloned()
        .collect();
    if listings.is_empty() {
        return None;
    }
    Some(ServerClient::Listings(janky_map_event_type(
        l,
        ListingEventData {
            item_id,
            world_id,
            listings,
        },
    )))
}

fn process_sales(
    l: Option<EventType<Arc<SaleEventData>>>,
    filter: &FilterPredicate,
    worlds: &WorldHelper,
) -> Option<ServerClient> {
    let l = l?;

    let sales: Vec<_> = l
        .as_ref()
        .sales
        .iter()
        .filter(|sale| filter.filter(worlds, *sale))
        .cloned()
        .collect();
    if sales.is_empty() {
        return None;
    }
    Some(ServerClient::Sales(janky_map_event_type(
        l,
        SaleEventData { sales },
    )))
}

async fn handle_socket(
    socket: WebSocket,
    events: EventReceivers,
    world_cache: Arc<WorldHelper>,
    db: UltrosDb,
    user: Option<AuthDiscordUser>,
    token: CancellationToken,
    list_sync: ListSync,
) -> Result<(), Box<dyn Error>> {
    let EventReceivers {
        retainers: _,
        listings,
        alerts: _,
        retainer_undercut: _,
        history,
        lists,
        list_docs,
    } = events;
    let socket_id = NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed);
    let (mut sender, mut receiver) = socket.split();
    let mut subscriptions = SelectAll::<BoxStream<ServerClient>>::new();
    let active_subscriptions = Arc::new(Mutex::new(HashSet::new()));
    let mut next_subscription_id = 1u64;
    // Tracks which subscription_id this socket used to subscribe to each
    // list document, so an unsolicited resync (ListDocError::MissingHistory)
    // can be routed back to the client's handler for that list — the
    // frontend dispatches `ListDocSubscribed` strictly by subscription_id.
    let mut list_doc_subscriptions: std::collections::HashMap<i32, u64> =
        std::collections::HashMap::new();
    subscriptions.push(Box::pin(futures::stream::pending()));
    sender
        .send(Message::Text(
            serde_json::to_string(&ServerClient::SocketConnected)?.into(),
        ))
        .await?;
    // sender.send(Message::Ping(vec![1, 2, 3, 4])).await?;

    info!("socket upgraded, starting.");
    // Set when the loop exits for shutdown rather than client disconnect; the
    // close frame goes out after the loop, once the select's borrows are gone.
    let mut shutting_down = false;
    loop {
        // Subscriptions can idle indefinitely, so without the shutdown token
        // this await keeps axum's graceful shutdown pending forever.
        match until_shutdown(&token, select(receiver.next(), subscriptions.next())).await {
            None => {
                shutting_down = true;
                break;
            }
            Some(selected) => match selected {
                Either::Left((Some(msg), _b)) => {
                    info!("Received message {msg:?}");
                    if let Ok(msg) = msg {
                        match msg {
                            Message::Text(text) => {
                                let msg: ClientMessage = serde_json::from_str(&text)?;
                                match msg {
                                    ClientMessage::AddSubscribe {
                                        subscription_id,
                                        filter,
                                        msg_type,
                                    } => {
                                        let subscription_id =
                                            subscription_id.unwrap_or_else(|| {
                                                let id = next_subscription_id;
                                                next_subscription_id += 1;
                                                id
                                            });
                                        if !activate_subscription(
                                            &active_subscriptions,
                                            subscription_id,
                                        ) {
                                            sender
                                            .send(Message::Text(
                                                serde_json::to_string(&ServerClient::Error {
                                                    message: format!(
                                                        "too many active subscriptions, max is {MAX_SUBSCRIPTIONS_PER_SOCKET}"
                                                    ),
                                                })?
                                                .into(),
                                            ))
                                            .await?;
                                            continue;
                                        }
                                        match msg_type {
                                            SocketMessageType::Listings => {
                                                let l_worlds = world_cache.clone();
                                                let active = active_subscriptions.clone();
                                                let stream =
                                                    BroadcastStream::new(listings.resubscribe())
                                                        .map(move |map| {
                                                            if !is_subscription_active(
                                                                &active,
                                                                subscription_id,
                                                            ) {
                                                                return None;
                                                            }
                                                            match map {
                                                                Ok(map) => {
                                                                    let filter = &filter;
                                                                    let worlds = &l_worlds;
                                                                    wrap_subscription_event(
                                                                        subscription_id,
                                                                        process_listings(
                                                                            Some(map),
                                                                            filter,
                                                                            worlds,
                                                                        ),
                                                                    )
                                                                }
                                                                Err(_) => {
                                                                    Some(ServerClient::Stale {
                                                                        subscription_id,
                                                                    })
                                                                }
                                                            }
                                                        })
                                                        .filter_map(move |f| async move { f });

                                                subscriptions.push(Box::pin(stream));
                                            }
                                            SocketMessageType::Sales => {
                                                let s_worlds = world_cache.clone();
                                                let active = active_subscriptions.clone();
                                                info!(
                                                    "Adding sales subscription with filter {filter:?}"
                                                );
                                                let stream =
                                                    BroadcastStream::new(history.resubscribe())
                                                        .map(move |map| {
                                                            if !is_subscription_active(
                                                                &active,
                                                                subscription_id,
                                                            ) {
                                                                return None;
                                                            }
                                                            match map {
                                                                Ok(map) => {
                                                                    let filter = &filter;
                                                                    let worlds = &s_worlds;
                                                                    wrap_subscription_event(
                                                                        subscription_id,
                                                                        process_sales(
                                                                            Some(map),
                                                                            filter,
                                                                            worlds,
                                                                        ),
                                                                    )
                                                                }
                                                                Err(_) => {
                                                                    Some(ServerClient::Stale {
                                                                        subscription_id,
                                                                    })
                                                                }
                                                            }
                                                        })
                                                        .filter_map(move |l| async move { l });

                                                subscriptions.push(Box::pin(stream));
                                            }
                                        }
                                        sender
                                            .send(Message::Text(
                                                serde_json::to_string(&ServerClient::Subscribed {
                                                    subscription_id,
                                                })?
                                                .into(),
                                            ))
                                            .await?;
                                    }
                                    ClientMessage::Unsubscribe { subscription_id } => {
                                        deactivate_subscription(
                                            &active_subscriptions,
                                            subscription_id,
                                        );
                                        list_doc_subscriptions
                                            .retain(|_list_id, id| *id != subscription_id);
                                        sender
                                            .send(Message::Text(
                                                serde_json::to_string(
                                                    &ServerClient::Unsubscribed { subscription_id },
                                                )?
                                                .into(),
                                            ))
                                            .await?;
                                    }
                                    ClientMessage::SubscribeList {
                                        subscription_id,
                                        list_id,
                                    } => {
                                        let subscription_id =
                                            subscription_id.unwrap_or_else(|| {
                                                let id = next_subscription_id;
                                                next_subscription_id += 1;
                                                id
                                            });
                                        if !activate_subscription(
                                            &active_subscriptions,
                                            subscription_id,
                                        ) {
                                            sender
                                            .send(Message::Text(
                                                serde_json::to_string(&ServerClient::Error {
                                                    message: format!(
                                                        "too many active subscriptions, max is {MAX_SUBSCRIPTIONS_PER_SOCKET}"
                                                    ),
                                                })?
                                                .into(),
                                            ))
                                            .await?;
                                            continue;
                                        }
                                        let user_id =
                                            user.as_ref().map(|u| u.id as i64).unwrap_or(0);
                                        let permission =
                                            db.get_permission(list_id, user_id).await?;
                                        if permission >= ListPermission::Read {
                                            let active = active_subscriptions.clone();
                                            let stream = BroadcastStream::new(lists.resubscribe())
                                                .filter_map(move |l| {
                                                    let active = active.clone();
                                                    async move {
                                                        if !is_subscription_active(
                                                            &active,
                                                            subscription_id,
                                                        ) {
                                                            return None;
                                                        }
                                                        let l = match l {
                                                            Ok(l) => l,
                                                            Err(_) => {
                                                                return Some(ServerClient::Stale {
                                                                    subscription_id,
                                                                });
                                                            }
                                                        };
                                                        let id = match &l {
                                                            crate::event::EventType::Add(inner)
                                                            | crate::event::EventType::Remove(
                                                                inner,
                                                            )
                                                            | crate::event::EventType::Update(
                                                                inner,
                                                            ) => match inner.as_ref() {
                                                                ListEventData::List(l) => l.id,
                                                                ListEventData::ListItem(l) => {
                                                                    l.list_id
                                                                }
                                                                ListEventData::Activity(a) => {
                                                                    a.list_id
                                                                }
                                                            },
                                                        };
                                                        if id == list_id {
                                                            let event = match l {
                                                                EventType::Add(a) => {
                                                                    WEvent::Added((*a).clone())
                                                                }
                                                                EventType::Remove(r) => {
                                                                    WEvent::Removed((*r).clone())
                                                                }
                                                                EventType::Update(u) => {
                                                                    WEvent::Updated((*u).clone())
                                                                }
                                                            };
                                                            wrap_subscription_event(
                                                                subscription_id,
                                                                Some(ServerClient::ListUpdate(
                                                                    event,
                                                                )),
                                                            )
                                                        } else {
                                                            None
                                                        }
                                                    }
                                                });
                                            subscriptions.push(Box::pin(stream));
                                            sender
                                                .send(Message::Text(
                                                    serde_json::to_string(
                                                        &ServerClient::Subscribed {
                                                            subscription_id,
                                                        },
                                                    )?
                                                    .into(),
                                                ))
                                                .await?;
                                        } else {
                                            deactivate_subscription(
                                                &active_subscriptions,
                                                subscription_id,
                                            );
                                            sender
                                                .send(Message::Text(
                                                    serde_json::to_string(&ServerClient::Error {
                                                        message:
                                                            "not authorized to subscribe to list"
                                                                .to_string(),
                                                    })?
                                                    .into(),
                                                ))
                                                .await?;
                                        }
                                    }
                                    ClientMessage::SubscribeListDoc {
                                        subscription_id,
                                        list_id,
                                        version,
                                    } => {
                                        let subscription_id =
                                            subscription_id.unwrap_or_else(|| {
                                                let id = next_subscription_id;
                                                next_subscription_id += 1;
                                                id
                                            });
                                        if !activate_subscription(
                                            &active_subscriptions,
                                            subscription_id,
                                        ) {
                                            sender
                                            .send(Message::Text(
                                                serde_json::to_string(&ServerClient::Error {
                                                    message: format!(
                                                        "too many active subscriptions, max is {MAX_SUBSCRIPTIONS_PER_SOCKET}"
                                                    ),
                                                })?
                                                .into(),
                                            ))
                                            .await?;
                                            continue;
                                        }
                                        let user_id =
                                            user.as_ref().map(|u| u.id as i64).unwrap_or(0);
                                        // Take the relay receiver BEFORE computing the
                                        // snapshot: subscribe_payload's await can yield
                                        // to another socket's merge in between. If we
                                        // resubscribed only after the snapshot came
                                        // back, an update merged during that await
                                        // would be missing from both the snapshot and
                                        // the relay — silently lost forever. Taking the
                                        // receiver first means the worst case is a
                                        // duplicate: an update already reflected in the
                                        // snapshot also arrives over the relay, which is
                                        // harmless because merging a CRDT update twice
                                        // is idempotent.
                                        let doc_relay = list_docs.resubscribe();
                                        match list_sync
                                            .subscribe_payload(list_id, user_id, &version)
                                            .await
                                        {
                                            Ok((server_version, payload)) => {
                                                list_doc_subscriptions
                                                    .insert(list_id, subscription_id);
                                                let active = active_subscriptions.clone();
                                                let stream = BroadcastStream::new(doc_relay)
                                                    .filter_map(move |event| {
                                                        let active = active.clone();
                                                        async move {
                                                            if !is_subscription_active(
                                                                &active,
                                                                subscription_id,
                                                            ) {
                                                                return None;
                                                            }
                                                            let event = match event {
                                                                Ok(event) => event,
                                                                Err(_) => {
                                                                    return Some(
                                                                        ServerClient::Stale {
                                                                            subscription_id,
                                                                        },
                                                                    );
                                                                }
                                                            };
                                                            let update = relay_for(
                                                                event.as_ref(),
                                                                list_id,
                                                                socket_id,
                                                            )?;
                                                            wrap_subscription_event(
                                                                subscription_id,
                                                                Some(ServerClient::ListDocUpdate {
                                                                    list_id,
                                                                    update,
                                                                }),
                                                            )
                                                        }
                                                    });
                                                subscriptions.push(Box::pin(stream));
                                                sender
                                                    .send(Message::Text(
                                                        serde_json::to_string(
                                                            &ServerClient::ListDocSubscribed {
                                                                subscription_id,
                                                                list_id,
                                                                version: server_version,
                                                                payload,
                                                            },
                                                        )?
                                                        .into(),
                                                    ))
                                                    .await?;
                                            }
                                            Err(e) => {
                                                deactivate_subscription(
                                                    &active_subscriptions,
                                                    subscription_id,
                                                );
                                                sender
                                                    .send(Message::Text(
                                                        serde_json::to_string(&scoped_event(
                                                            subscription_id,
                                                            ServerClient::Error {
                                                                message: format!(
                                                                    "list {list_id}: {e}"
                                                                ),
                                                            },
                                                        ))?
                                                        .into(),
                                                    ))
                                                    .await?;
                                            }
                                        }
                                    }
                                    ClientMessage::ListDocUpdate { list_id, update } => {
                                        // Errors for a list-doc update are wrapped in
                                        // the SubscriptionEvent of whichever
                                        // SubscribeListDoc subscription this socket
                                        // has for the list (spec section 5), so only
                                        // that subscription's handler sees them — a
                                        // bare Error fans out to every handler on the
                                        // socket (see dispatch_message in the
                                        // frontend). If this socket has no such
                                        // subscription, fall back to a bare Error.
                                        let resync_subscription_id =
                                            list_doc_subscriptions.get(&list_id).copied();
                                        let Some(user) = user.as_ref() else {
                                            let error = ServerClient::Error {
                                                message: "sign in to edit lists".to_string(),
                                            };
                                            let payload = match resync_subscription_id {
                                                Some(subscription_id) => {
                                                    scoped_event(subscription_id, error)
                                                }
                                                None => error,
                                            };
                                            sender
                                                .send(Message::Text(
                                                    serde_json::to_string(&payload)?.into(),
                                                ))
                                                .await?;
                                            continue;
                                        };
                                        let actor =
                                            Actor::from_user(user, Origin::Socket(socket_id));
                                        if let Err(e) =
                                            list_sync.apply_update(list_id, &actor, &update).await
                                        {
                                            match e {
                                                ListDocError::MissingHistory => {
                                                    let user_id = user.id as i64;
                                                    // Only compute a resync snapshot
                                                    // when we actually have a
                                                    // subscription id to route it to
                                                    // (M1) — otherwise the snapshot
                                                    // would be built and discarded.
                                                    match resync_subscription_id {
                                                        Some(subscription_id) => {
                                                            match list_sync
                                                                .subscribe_payload(
                                                                    list_id,
                                                                    user_id,
                                                                    &[],
                                                                )
                                                                .await
                                                            {
                                                                Ok((server_version, payload)) => {
                                                                    sender
                                                                    .send(Message::Text(
                                                                        serde_json::to_string(
                                                                            &ServerClient::ListDocSubscribed {
                                                                                subscription_id,
                                                                                list_id,
                                                                                version: server_version,
                                                                                payload,
                                                                            },
                                                                        )?
                                                                        .into(),
                                                                    ))
                                                                    .await?;
                                                                }
                                                                Err(e) => {
                                                                    sender
                                                                    .send(Message::Text(
                                                                        serde_json::to_string(
                                                                            &scoped_event(
                                                                                subscription_id,
                                                                                ServerClient::Error {
                                                                                    message: format!(
                                                                                        "list {list_id}: {e}"
                                                                                    ),
                                                                                },
                                                                            ),
                                                                        )?
                                                                        .into(),
                                                                    ))
                                                                    .await?;
                                                                }
                                                            }
                                                        }
                                                        None => {
                                                            // No active SubscribeListDoc for this
                                                            // list on this socket to route the
                                                            // resync to; report the merge failure
                                                            // bare, as before.
                                                            sender
                                                            .send(Message::Text(
                                                                serde_json::to_string(
                                                                    &ServerClient::Error {
                                                                        message: format!(
                                                                            "list {list_id}: {}",
                                                                            ListDocError::MissingHistory
                                                                        ),
                                                                    },
                                                                )?
                                                                .into(),
                                                            ))
                                                            .await?;
                                                        }
                                                    }
                                                }
                                                e => {
                                                    let error = ServerClient::Error {
                                                        message: format!("list {list_id}: {e}"),
                                                    };
                                                    let payload = match resync_subscription_id {
                                                        Some(subscription_id) => {
                                                            scoped_event(subscription_id, error)
                                                        }
                                                        None => error,
                                                    };
                                                    sender
                                                        .send(Message::Text(
                                                            serde_json::to_string(&payload)?.into(),
                                                        ))
                                                        .await?;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Message::Binary(_) => {
                                info!("binary data received");
                            }
                            Message::Ping(ping) => {
                                info!("{ping:?}");
                                sender.send(Message::Pong(ping)).await?;
                            }
                            Message::Pong(pong) => {
                                info!("{pong:?}");
                            }
                            Message::Close(_close) => {
                                info!("real time socket closed")
                            }
                        }
                    } else {
                        // client disconnected
                        info!("websocket disconnect");
                        return Ok(());
                    };
                }
                Either::Right((Some(right), _l)) => {
                    info!("Sending websocket message {right:?}");
                    sender
                        .send(Message::Text(serde_json::to_string(&right)?.into()))
                        .await?;
                }
                Either::Left((left, _l)) => {
                    info!("Received none: {left:?}");
                    break;
                }
                Either::Right((right, _r)) => {
                    info!("Right {right:?}");
                    break;
                }
            },
        };
    }

    if shutting_down {
        info!("server shutting down, closing real time socket");
        let _ = sender.send(Message::Close(None)).await;
    }

    Ok(())
}

fn activate_subscription(active_subscriptions: &Arc<Mutex<HashSet<u64>>>, id: u64) -> bool {
    let Ok(mut active) = active_subscriptions.lock() else {
        return false;
    };
    if !active.contains(&id) && active.len() >= MAX_SUBSCRIPTIONS_PER_SOCKET {
        return false;
    }
    active.insert(id);
    true
}

fn deactivate_subscription(active_subscriptions: &Arc<Mutex<HashSet<u64>>>, id: u64) {
    if let Ok(mut active) = active_subscriptions.lock() {
        active.remove(&id);
    }
}

fn is_subscription_active(active_subscriptions: &Arc<Mutex<HashSet<u64>>>, id: u64) -> bool {
    active_subscriptions
        .lock()
        .map(|active| active.contains(&id))
        .unwrap_or(false)
}

fn wrap_subscription_event(
    subscription_id: u64,
    event: Option<ServerClient>,
) -> Option<ServerClient> {
    event.map(|event| ServerClient::SubscriptionEvent {
        subscription_id,
        event: Box::new(event),
    })
}

/// Wraps a single `ServerClient` (never `None`) in its subscription's
/// `SubscriptionEvent`, for call sites that always have a value to send —
/// mainly error replies scoped to one list-doc subscription (spec section 5).
fn scoped_event(subscription_id: u64, event: ServerClient) -> ServerClient {
    ServerClient::SubscriptionEvent {
        subscription_id,
        event: Box::new(event),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ListDocEvent;

    #[test]
    fn relay_skips_other_lists_and_the_sending_socket() {
        let event = ListDocEvent {
            list_id: 9,
            update: vec![1, 2, 3],
            origin_socket: Some(7),
        };
        assert_eq!(relay_for(&event, 9, 8), Some(vec![1, 2, 3]));
        assert_eq!(relay_for(&event, 9, 7), None, "the sender already has it");
        assert_eq!(relay_for(&event, 10, 8), None, "another list");
        let server_side = ListDocEvent {
            list_id: 9,
            update: vec![4],
            origin_socket: None,
        };
        assert_eq!(relay_for(&server_side, 9, 7), Some(vec![4]));
    }
}
