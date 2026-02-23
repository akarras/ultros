use std::{error::Error, sync::Arc};

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
use tracing::{error, info};
use ultros_api_types::websocket::{
    ClientMessage, FilterPredicate, ListEventData, ListingEventData, SaleEventData, ServerClient,
    SocketMessageType,
};
use ultros_api_types::{websocket::EventType as WEvent, world_helper::WorldHelper};

use crate::event::{EventReceivers, EventType};
use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;
use ultros_api_types::list::ListPermission;
use ultros_db::UltrosDb;

pub(crate) async fn real_time_data(
    ws: WebSocketUpgrade,
    user: Result<AuthDiscordUser, ApiError>,
    State(events): State<EventReceivers>,
    State(worlds): State<Arc<WorldHelper>>,
    State(db): State<UltrosDb>,
) -> Response {
    let user = user.ok();
    info!("Handling websocket");
    ws.on_upgrade(move |websocket| async move {
        info!("Upgrading websocket");
        if let Err(e) = handle_socket(websocket, events, worlds, db, user).await {
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
) -> Result<(), Box<dyn Error>> {
    let EventReceivers {
        retainers: _,
        listings,
        alerts: _,
        retainer_undercut: _,
        history,
        lists,
    } = events;
    let (mut sender, mut receiver) = socket.split();
    let mut subscriptions = SelectAll::<BoxStream<ServerClient>>::new();
    subscriptions.push(Box::pin(futures::stream::pending()));
    sender
        .send(Message::Text(
            serde_json::to_string(&ServerClient::SocketConnected)?.into(),
        ))
        .await?;
    // sender.send(Message::Ping(vec![1, 2, 3, 4])).await?;

    info!("socket upgraded, starting.");
    loop {
        match select(receiver.next(), subscriptions.next()).await {
            Either::Left((Some(msg), _b)) => {
                info!("Received message {msg:?}");
                if let Ok(msg) = msg {
                    match msg {
                        Message::Text(text) => {
                            let msg: ClientMessage = serde_json::from_str(&text)?;
                            match msg {
                                ClientMessage::AddSubscribe { filter, msg_type } => {
                                    match msg_type {
                                        SocketMessageType::Listings => {
                                            let l_worlds = world_cache.clone();
                                            let stream =
                                                BroadcastStream::new(listings.resubscribe())
                                                    .map(move |map| {
                                                        let filter = &filter;
                                                        let worlds = &l_worlds;
                                                        process_listings(map.ok(), filter, worlds)
                                                    })
                                                    .filter_map(move |f| async move { f });

                                            subscriptions.push(Box::pin(stream));
                                        }
                                        SocketMessageType::Sales => {
                                            let s_worlds = world_cache.clone();
                                            info!(
                                                "Adding sales subscription with filter {filter:?}"
                                            );
                                            let stream =
                                                BroadcastStream::new(history.resubscribe())
                                                    .map(move |map| {
                                                        let filter = &filter;
                                                        let worlds = &s_worlds;
                                                        process_sales(map.ok(), filter, worlds)
                                                    })
                                                    .filter_map(move |l| async move { l });

                                            subscriptions.push(Box::pin(stream));
                                        }
                                    }
                                }
                                ClientMessage::SubscribeList { list_id } => {
                                    let user_id = user.as_ref().map(|u| u.id as i64).unwrap_or(0);
                                    let permission = db.get_permission(list_id, user_id).await?;
                                    if permission >= ListPermission::Read {
                                        let stream = BroadcastStream::new(lists.resubscribe())
                                            .filter_map(move |l| async move {
                                                let l = l.ok()?;
                                                let id = match &l {
                                                    crate::event::EventType::Add(inner)
                                                    | crate::event::EventType::Remove(inner)
                                                    | crate::event::EventType::Update(inner) => {
                                                        match inner.as_ref() {
                                                            ListEventData::List(l) => l.id,
                                                            ListEventData::ListItem(l) => l.list_id,
                                                        }
                                                    }
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
                                                    Some(ServerClient::ListUpdate(event))
                                                } else {
                                                    None
                                                }
                                            });
                                        subscriptions.push(Box::pin(stream));
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
        };
    }

    Ok(())
}
