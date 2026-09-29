use std::{
    collections::HashMap,
    error::Error,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
};

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
};

use futures::{
    SinkExt, Stream, StreamExt,
    future::{Either, select},
    stream::BoxStream,
};

use tokio_stream::{StreamMap, wrappers::BroadcastStream};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use ultros_api_types::websocket::{
    ClientMessage, FilterPredicate, ListEventData, ListingEventData, SaleEventData, ServerClient,
    SocketMessageType,
};
use ultros_api_types::{websocket::EventType as WEvent, world_helper::WorldHelper};

use crate::event::{EventReceivers, EventType, ListDocEvent, NotificationEvent};
use crate::lists::{ListSync, Origin};
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

/// Relays fired alerts to the socket that owns them: `Err(_)` (lag) maps to
/// `Stale`, `Ok(e)` for a different owner is dropped, and `Ok(e)` for this
/// owner is wrapped in its subscription's `SubscriptionEvent`.
fn notification_relay(
    receiver: tokio::sync::broadcast::Receiver<EventType<Arc<NotificationEvent>>>,
    subscription_id: u64,
    owner: i64,
) -> BoxStream<'static, ServerClient> {
    Box::pin(BroadcastStream::new(receiver).filter_map(move |event| {
        let result = match event {
            Err(_) => Some(ServerClient::Stale { subscription_id }),
            Ok(event) if event.as_ref().owner == owner => Some(scoped_event(
                subscription_id,
                ServerClient::Notification(event.as_ref().event.clone()),
            )),
            Ok(_) => None,
        };
        async move { result }
    }))
}

fn list_doc_relay<F, A>(
    receiver: tokio::sync::broadcast::Receiver<EventType<Arc<ListDocEvent>>>,
    subscription_id: u64,
    list_id: i32,
    socket_id: u64,
    mut authorize: A,
) -> BoxStream<'static, ServerClient>
where
    A: FnMut(ServerClient) -> F + Send + 'static,
    F: std::future::Future<Output = Option<ServerClient>> + Send + 'static,
{
    Box::pin(BroadcastStream::new(receiver).filter_map(move |event| {
        let lagged = event.is_err();
        let authorization = event.ok().and_then(|event| {
            relay_for(event.as_ref(), list_id, socket_id)
                .map(|update| authorize(ServerClient::ListDocUpdate { list_id, update }))
        });
        async move {
            if lagged {
                Some(ServerClient::Stale { subscription_id })
            } else if let Some(authorization) = authorization {
                authorization.await
            } else {
                None
            }
        }
    }))
}

/// A `SubscribeList` for a list that does not exist is answered on that one
/// subscription; the socket stays up for everything else it carries (market
/// feeds, notifications, other lists). Propagating the lookup error instead
/// closed the whole socket and logged a fatal "List not found" for what is
/// ordinary client state: a page still subscribed to a list deleted from
/// another tab, or a stale id replayed on reconnect. Other lookup failures
/// (database down) are still fatal for the socket.
fn list_lookup_rejection(error: &anyhow::Error, list_id: i32) -> Option<String> {
    use ultros_db::lists::ListError;
    match error.downcast_ref::<ListError>() {
        Some(ListError::NotFound) => Some(format!("list {list_id}: {}", ListError::NotFound)),
        _ => None,
    }
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
        notifications,
    } = events;
    let socket_id = NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed);
    let (mut sender, mut receiver) = socket.split();
    let mut subscriptions = SocketSubscriptions::default();
    let mut next_subscription_id = 1u64;
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
                                        if !subscriptions.begin(subscription_id) {
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
                                                let stream =
                                                    BroadcastStream::new(listings.resubscribe())
                                                        .map(move |map| match map {
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
                                                            Err(_) => Some(ServerClient::Stale {
                                                                subscription_id,
                                                            }),
                                                        })
                                                        .filter_map(move |f| async move { f });

                                                subscriptions
                                                    .insert(subscription_id, Box::pin(stream));
                                            }
                                            SocketMessageType::Sales => {
                                                let s_worlds = world_cache.clone();
                                                info!(
                                                    "Adding sales subscription with filter {filter:?}"
                                                );
                                                let stream =
                                                    BroadcastStream::new(history.resubscribe())
                                                        .map(move |map| match map {
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
                                                            Err(_) => Some(ServerClient::Stale {
                                                                subscription_id,
                                                            }),
                                                        })
                                                        .filter_map(move |l| async move { l });

                                                subscriptions
                                                    .insert(subscription_id, Box::pin(stream));
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
                                        subscriptions.remove(subscription_id);
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
                                        if !subscriptions.begin(subscription_id) {
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
                                            match db.get_permission(list_id, user_id).await {
                                                Ok(permission) => permission,
                                                Err(error) => {
                                                    let Some(message) =
                                                        list_lookup_rejection(&error, list_id)
                                                    else {
                                                        return Err(error.into());
                                                    };
                                                    info!(list_id, "rejecting list subscription");
                                                    subscriptions.remove(subscription_id);
                                                    sender
                                                        .send(Message::Text(
                                                            serde_json::to_string(
                                                                &ServerClient::Error { message },
                                                            )?
                                                            .into(),
                                                        ))
                                                        .await?;
                                                    continue;
                                                }
                                            };
                                        if permission >= ListPermission::Read {
                                            let relay_db = db.clone();
                                            let stream = BroadcastStream::new(lists.resubscribe())
                                                .filter_map(move |l| {
                                                    let db = relay_db.clone();
                                                    async move {
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
                                                            authorize_list_event(
                                                                &db,
                                                                subscription_id,
                                                                list_id,
                                                                user_id,
                                                                ServerClient::ListUpdate(event),
                                                            )
                                                            .await
                                                        } else {
                                                            None
                                                        }
                                                    }
                                                });
                                            subscriptions.insert(subscription_id, Box::pin(stream));
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
                                            subscriptions.remove(subscription_id);
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
                                        if !subscriptions.begin(subscription_id) {
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
                                                subscriptions
                                                    .list_docs
                                                    .insert(list_id, subscription_id);
                                                let relay_db = db.clone();
                                                let stream = list_doc_relay(
                                                    doc_relay,
                                                    subscription_id,
                                                    list_id,
                                                    socket_id,
                                                    move |event| {
                                                        let db = relay_db.clone();
                                                        async move {
                                                            authorize_list_event(
                                                                &db,
                                                                subscription_id,
                                                                list_id,
                                                                user_id,
                                                                event,
                                                            )
                                                            .await
                                                        }
                                                    },
                                                );
                                                subscriptions.insert(subscription_id, stream);
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
                                                subscriptions.remove(subscription_id);
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
                                            subscriptions.list_docs.get(&list_id).copied();
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
                                        let actor = user.actor(Origin::Socket(socket_id));
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
                                    ClientMessage::SubscribeNotifications { subscription_id } => {
                                        let subscription_id =
                                            subscription_id.unwrap_or_else(|| {
                                                let id = next_subscription_id;
                                                next_subscription_id += 1;
                                                id
                                            });
                                        if !subscriptions.begin(subscription_id) {
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
                                        match authorize_notifications(
                                            user.as_ref(),
                                            subscription_id,
                                        ) {
                                            Ok(owner) => {
                                                let stream = notification_relay(
                                                    notifications.resubscribe(),
                                                    subscription_id,
                                                    owner,
                                                );
                                                subscriptions.insert(subscription_id, stream);
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
                                            }
                                            Err(error) => {
                                                subscriptions.remove(subscription_id);
                                                sender
                                                    .send(Message::Text(
                                                        serde_json::to_string(&error)?.into(),
                                                    ))
                                                    .await?;
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
                                info!("real time socket closed");
                                break;
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

/// A socket owns exactly one stream (and any pending authorization future) per
/// subscription ID. There are no detached tasks or shared ID-based active flags:
/// replacing/removing an ID drops that generation synchronously, so its pending
/// database result can neither emit data nor retire a later generation.
#[derive(Default)]
struct SocketSubscriptions {
    streams: StreamMap<u64, BoxStream<'static, ServerClient>>,
    // Route unsolicited document resync replies to the current handler only.
    list_docs: HashMap<i32, u64>,
}

impl SocketSubscriptions {
    /// Retire the previous generation before even starting the new handshake.
    /// A failed handshake therefore cannot leave the old relay or route alive.
    fn begin(&mut self, id: u64) -> bool {
        if !self.streams.contains_key(&id) && self.streams.len() >= MAX_SUBSCRIPTIONS_PER_SOCKET {
            return false;
        }
        self.remove(id);
        true
    }

    fn insert(&mut self, id: u64, stream: BoxStream<'static, ServerClient>) {
        self.streams.insert(id, stream);
    }

    fn remove(&mut self, id: u64) {
        self.streams.remove(&id);
        self.list_docs
            .retain(|_, subscription_id| *subscription_id != id);
    }
}

impl Stream for SocketSubscriptions {
    type Item = ServerClient;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.streams).poll_next(cx);
        // StreamMap also removes exhausted/closed sources during polling.
        this.list_docs.retain(|_, id| this.streams.contains_key(id));
        match result {
            Poll::Ready(Some((id, event))) => {
                // A relay's scoped error is a terminal authorization failure.
                // Drop its receiver and future before sending the error frame.
                if matches!(&event, ServerClient::SubscriptionEvent { event, .. }
                    if matches!(event.as_ref(), ServerClient::Error { .. }))
                {
                    this.remove(id);
                }
                Poll::Ready(Some(event))
            }
            // An empty subscription collection must not close the socket. The
            // client receiver remains polled and can install the next stream.
            Poll::Ready(None) | Poll::Pending => Poll::Pending,
        }
    }
}

/// A successful handshake is not a permanent grant. This future is owned by
/// its relay stream and is canceled on replacement, unsubscribe or socket close.
async fn authorize_list_event(
    db: &UltrosDb,
    subscription_id: u64,
    list_id: i32,
    user_id: i64,
    event: ServerClient,
) -> Option<ServerClient> {
    let permission = db.get_permission(list_id, user_id).await;
    Some(finish_list_authorization(
        subscription_id,
        list_id,
        permission,
        event,
    ))
}

fn finish_list_authorization(
    subscription_id: u64,
    list_id: i32,
    permission: anyhow::Result<ListPermission>,
    event: ServerClient,
) -> ServerClient {
    if matches!(permission, Ok(value) if value >= ListPermission::Read) {
        return scoped_event(subscription_id, event);
    }
    // Fail closed on lookup errors too. The owner drops only this relay when
    // it polls the error; unrelated lists and market subscriptions remain live.
    scoped_event(
        subscription_id,
        ServerClient::Error {
            message: format!("forbidden: no verified read access to list {list_id}"),
        },
    )
}

/// Anonymous sockets have no owner to address an inbox to, so the handshake
/// is rejected up front rather than installing a relay that would forward
/// nothing (mirrors `ClientMessage::ListDocUpdate`'s anonymous handling).
///
/// The error is boxed because `ServerClient` is large (its `ListDocSubscribed`
/// variant carries a version vector and a document payload); clippy's
/// `result_large_err` flags returning it by value.
fn authorize_notifications(
    user: Option<&AuthDiscordUser>,
    subscription_id: u64,
) -> Result<i64, Box<ServerClient>> {
    user.map(|u| u.id as i64).ok_or_else(|| {
        Box::new(scoped_event(
            subscription_id,
            ServerClient::Error {
                message: "sign in to receive notifications".to_string(),
            },
        ))
    })
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
    use futures::FutureExt;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::{broadcast, oneshot};
    use ultros_api_types::alert::AlertEvent;

    /// The reply uses the wording the client's `classify_error` already
    /// recognises as NotFound, so a still-subscribed page treats it like a
    /// deleted list rather than a lost connection.
    #[test]
    fn missing_list_subscription_is_answered_instead_of_closing_the_socket() {
        let missing = anyhow::Error::from(ultros_db::lists::ListError::NotFound);
        assert_eq!(
            list_lookup_rejection(&missing, 9).as_deref(),
            Some("list 9: List not found")
        );
        let outage = anyhow::anyhow!("connection reset by peer");
        assert!(list_lookup_rejection(&outage, 9).is_none());
    }

    fn notification_event(owner: i64, alert_id: i32) -> EventType<Arc<NotificationEvent>> {
        EventType::added(NotificationEvent {
            owner,
            event: AlertEvent {
                id: 1,
                alert_id,
                fired_at: chrono::DateTime::<chrono::Utc>::default(),
                item_id: 42,
                matched_listing_id: None,
                matched_price: Some(100),
                delivered: true,
                delivery_error: None,
                read_at: None,
                title: Some("Title".to_string()),
                body: Some("Body".to_string()),
                click_url: Some("/item/42".to_string()),
            },
        })
    }

    #[tokio::test]
    async fn notification_relay_delivers_only_the_owners_events() {
        let (bus, _) = broadcast::channel(16);
        let mut relay = notification_relay(bus.subscribe(), 11, 1);
        bus.send(notification_event(2, 99)).unwrap();
        bus.send(notification_event(1, 7)).unwrap();

        match relay.next().await {
            Some(ServerClient::SubscriptionEvent {
                subscription_id: 11,
                event,
            }) => match *event {
                ServerClient::Notification(e) => assert_eq!(e.alert_id, 7),
                other => panic!("expected Notification, got {other:?}"),
            },
            other => panic!("expected SubscriptionEvent, got {other:?}"),
        }
        assert!(
            relay.next().now_or_never().is_none(),
            "the other owner's event was dropped, not merely delayed"
        );
    }

    #[tokio::test]
    async fn notification_relay_reports_lag_as_stale_then_resumes() {
        let (bus, _) = broadcast::channel(2);
        let mut relay = notification_relay(bus.subscribe(), 11, 1);
        for i in 0..5 {
            bus.send(notification_event(1, i)).unwrap();
        }
        assert!(matches!(
            relay.next().await,
            Some(ServerClient::Stale {
                subscription_id: 11
            })
        ));
        // Whatever survived the ring after the lag keeps flowing.
        match relay.next().await {
            Some(ServerClient::SubscriptionEvent {
                subscription_id: 11,
                ..
            }) => {}
            other => panic!("expected the relay to resume, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn notification_relay_closed_bus_releases_stream_without_closing_socket() {
        let (bus, _) = broadcast::channel::<EventType<Arc<NotificationEvent>>>(16);
        let mut relay = notification_relay(bus.subscribe(), 11, 1);
        drop(bus);
        assert!(
            relay.next().await.is_none(),
            "a closed bus ends the relay stream"
        );

        // An empty subscription map must not look like EOF to the socket loop.
        let mut subscriptions = SocketSubscriptions::default();
        assert!(subscriptions.next().now_or_never().is_none());
    }

    #[test]
    fn anonymous_notification_subscribe_gets_scoped_error() {
        let error = match authorize_notifications(None, 11) {
            Err(error) => *error,
            Ok(owner) => panic!("anonymous socket should not be authorized, got owner {owner}"),
        };
        match error {
            ServerClient::SubscriptionEvent {
                subscription_id: 11,
                event,
            } => match *event {
                ServerClient::Error { message } => {
                    assert_eq!(message, "sign in to receive notifications")
                }
                other => panic!("expected Error, got {other:?}"),
            },
            other => panic!("expected a scoped SubscriptionEvent, got {other:?}"),
        }

        let user = AuthDiscordUser {
            id: 42,
            name: "someone".to_string(),
            avatar_url: String::new(),
        };
        match authorize_notifications(Some(&user), 11) {
            Ok(owner) => assert_eq!(owner, 42),
            Err(e) => panic!("an authenticated user should be authorized, got {e:?}"),
        }
    }

    #[test]
    fn notification_relay_respects_subscription_limit() {
        let (bus, _) = broadcast::channel(16);
        let mut subscriptions = SocketSubscriptions::default();
        for id in 0..MAX_SUBSCRIPTIONS_PER_SOCKET as u64 {
            assert!(subscriptions.begin(id));
            subscriptions.insert(id, notification_relay(bus.subscribe(), id, 1));
        }
        assert_eq!(subscriptions.streams.len(), MAX_SUBSCRIPTIONS_PER_SOCKET);
        assert!(
            !subscriptions.begin(MAX_SUBSCRIPTIONS_PER_SOCKET as u64),
            "at the cap, a brand new id is refused"
        );
        // Replacing an id already held is still allowed at the cap.
        assert!(subscriptions.begin(0));
        subscriptions.insert(0, notification_relay(bus.subscribe(), 0, 1));
        assert_eq!(subscriptions.streams.len(), MAX_SUBSCRIPTIONS_PER_SOCKET);
    }

    fn update(list_id: i32) -> EventType<Arc<ListDocEvent>> {
        EventType::Update(Arc::new(ListDocEvent {
            list_id,
            update: vec![99],
            origin_socket: Some(2),
        }))
    }

    fn install(
        subscriptions: &mut SocketSubscriptions,
        bus: &broadcast::Sender<EventType<Arc<ListDocEvent>>>,
        id: u64,
        calls: &Arc<AtomicUsize>,
    ) {
        assert!(subscriptions.begin(id));
        let calls = calls.clone();
        subscriptions.insert(
            id,
            list_doc_relay(bus.subscribe(), id, 7, 1, move |event| {
                calls.fetch_add(1, Ordering::Relaxed);
                futures::future::ready(Some(finish_list_authorization(
                    id,
                    7,
                    Ok(ListPermission::Read),
                    event,
                )))
            }),
        );
        subscriptions.list_docs.insert(7, id);
    }

    #[tokio::test]
    async fn repeated_resubscribe_and_navigation_keep_one_receiver_and_authorization() {
        let (bus, _) = broadcast::channel(16);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut subscriptions = SocketSubscriptions::default();
        for _ in 0..200 {
            install(&mut subscriptions, &bus, 11, &calls);
            assert_eq!(subscriptions.streams.len(), 1);
            assert_eq!(bus.receiver_count(), 1, "retirement needs no polling");
        }
        bus.send(update(7)).unwrap();
        assert!(matches!(subscriptions.next().await,
            Some(ServerClient::SubscriptionEvent { subscription_id: 11, event })
                if matches!(*event, ServerClient::ListDocUpdate { .. })));
        assert!(subscriptions.next().now_or_never().is_none());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        for id in 100..300 {
            subscriptions.remove(11);
            install(&mut subscriptions, &bus, id, &calls);
            subscriptions.remove(id);
            assert!(subscriptions.streams.is_empty());
            assert!(subscriptions.list_docs.is_empty());
            assert_eq!(bus.receiver_count(), 0);
        }
        // No subscriptions means idle, not EOF. A later navigation still works.
        assert!(subscriptions.next().now_or_never().is_none());
        install(&mut subscriptions, &bus, 11, &calls);
        bus.send(update(7)).unwrap();
        assert!(subscriptions.next().await.is_some());
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn replacement_cancels_pending_authorization_before_id_reuse() {
        for permission in [
            Ok(ListPermission::Read),
            Ok(ListPermission::None),
            Err(anyhow::anyhow!("lookup failed")),
        ] {
            let (bus, _) = broadcast::channel(16);
            let (reply, response) = oneshot::channel();
            let mut response = Some(response);
            let mut subscriptions = SocketSubscriptions::default();
            assert!(subscriptions.begin(11));
            subscriptions.insert(
                11,
                list_doc_relay(bus.subscribe(), 11, 7, 1, move |event| {
                    let response = response.take().unwrap();
                    async move {
                        Some(finish_list_authorization(
                            11,
                            7,
                            response.await.unwrap(),
                            event,
                        ))
                    }
                }),
            );
            bus.send(update(7)).unwrap();
            assert!(subscriptions.next().now_or_never().is_none());
            let calls = Arc::new(AtomicUsize::new(0));
            install(&mut subscriptions, &bus, 11, &calls);
            assert!(
                reply.send(permission).is_err(),
                "old lookup future was dropped"
            );
            assert_eq!(bus.receiver_count(), 1);
            bus.send(update(7)).unwrap();
            assert!(matches!(subscriptions.next().await,
                Some(ServerClient::SubscriptionEvent { event, .. })
                    if matches!(*event, ServerClient::ListDocUpdate { .. })));
            assert_eq!(calls.load(Ordering::Relaxed), 1);
            assert_eq!(subscriptions.streams.len(), 1);
        }
    }

    #[tokio::test]
    async fn denial_and_lookup_failure_drop_only_the_affected_relay() {
        for permission in [
            Ok(ListPermission::None),
            Err(anyhow::anyhow!("lookup failed")),
        ] {
            let (bus, _) = broadcast::channel(16);
            let mut subscriptions = SocketSubscriptions::default();
            let mut permission = Some(permission);
            subscriptions.insert(
                11,
                list_doc_relay(bus.subscribe(), 11, 7, 1, move |event| {
                    futures::future::ready(Some(finish_list_authorization(
                        11,
                        7,
                        permission.take().unwrap(),
                        event,
                    )))
                }),
            );
            subscriptions.list_docs.insert(7, 11);
            subscriptions.insert(22, Box::pin(futures::stream::pending()));
            bus.send(update(7)).unwrap();
            assert!(matches!(subscriptions.next().await,
                Some(ServerClient::SubscriptionEvent { subscription_id: 11, event })
                    if matches!(*event, ServerClient::Error { .. })));
            assert_eq!(bus.receiver_count(), 0);
            assert!(subscriptions.list_docs.is_empty());
            assert!(subscriptions.streams.contains_key(&22));
            assert!(!subscriptions.streams.contains_key(&11));
        }
    }

    #[tokio::test]
    async fn lag_resync_replaces_receiver_and_preserves_updates_during_handshake() {
        let (bus, _) = broadcast::channel(2);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut subscriptions = SocketSubscriptions::default();
        install(&mut subscriptions, &bus, 11, &calls);
        for _ in 0..5 {
            bus.send(update(7)).unwrap();
        }
        assert!(matches!(
            subscriptions.next().await,
            Some(ServerClient::Stale {
                subscription_id: 11
            })
        ));
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert!(subscriptions.begin(11));
        let receiver = bus.subscribe();
        // The actual handshake acquires its receiver before awaiting a snapshot.
        bus.send(update(7)).unwrap();
        let relay_calls = calls.clone();
        subscriptions.insert(
            11,
            list_doc_relay(receiver, 11, 7, 1, move |event| {
                relay_calls.fetch_add(1, Ordering::Relaxed);
                futures::future::ready(Some(scoped_event(11, event)))
            }),
        );
        assert_eq!(bus.receiver_count(), 1);
        assert!(subscriptions.next().await.is_some());
        assert!(subscriptions.next().now_or_never().is_none());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn failed_handshake_id_reuse_and_limit_retire_routes_and_bound_streams() {
        let (bus, _) = broadcast::channel(16);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut subscriptions = SocketSubscriptions::default();
        install(&mut subscriptions, &bus, 11, &calls);
        assert!(subscriptions.begin(11));
        // A denied/failed handshake adds no replacement; the old one is gone.
        assert_eq!(bus.receiver_count(), 0);
        assert!(subscriptions.list_docs.is_empty());
        install(&mut subscriptions, &bus, 11, &calls);
        assert!(subscriptions.begin(11));
        subscriptions.insert(11, Box::pin(futures::stream::pending()));
        assert!(
            subscriptions.list_docs.is_empty(),
            "reuse for market data clears doc routing"
        );
        for id in 100..163 {
            install(&mut subscriptions, &bus, id, &calls);
        }
        assert_eq!(subscriptions.streams.len(), MAX_SUBSCRIPTIONS_PER_SOCKET);
        assert!(!subscriptions.begin(999));
        install(&mut subscriptions, &bus, 100, &calls);
        assert_eq!(subscriptions.streams.len(), MAX_SUBSCRIPTIONS_PER_SOCKET);
        drop(subscriptions);
        assert_eq!(bus.receiver_count(), 0, "socket close drops every relay");
    }

    #[test]
    fn socket_close_cancels_pending_permission_lookup() {
        let (bus, _) = broadcast::channel(16);
        let (reply, response) = oneshot::channel::<()>();
        let mut response = Some(response);
        let mut subscriptions = SocketSubscriptions::default();
        subscriptions.insert(
            11,
            list_doc_relay(bus.subscribe(), 11, 7, 1, move |_| {
                let response = response.take().unwrap();
                async move {
                    response.await.unwrap();
                    None
                }
            }),
        );
        bus.send(update(7)).unwrap();
        assert!(subscriptions.next().now_or_never().is_none());
        drop(subscriptions);
        assert!(reply.send(()).is_err());
        assert_eq!(bus.receiver_count(), 0);
    }

    #[test]
    fn closed_source_releases_its_route_without_closing_the_socket() {
        let (bus, _) = broadcast::channel(16);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut subscriptions = SocketSubscriptions::default();
        install(&mut subscriptions, &bus, 11, &calls);
        drop(bus);
        assert!(subscriptions.next().now_or_never().is_none());
        assert!(subscriptions.streams.is_empty());
        assert!(subscriptions.list_docs.is_empty());
    }

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
