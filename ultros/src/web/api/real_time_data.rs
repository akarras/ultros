use std::{error::Error, sync::Arc};

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};

use futures::{
    future::{select, Either},
    stream::{BoxStream, SelectAll},
    SinkExt, StreamExt,
};

use tokio_stream::wrappers::BroadcastStream;
use tracing::{error, info};
use ultros_api_types::websocket::{
    ClientMessage, FilterPredicate, ListingEventData, SaleEventData, ServerClient,
    SocketMessageType,
};
use ultros_api_types::{websocket::EventType as WEvent, world_helper::WorldHelper};

use crate::event::{EventReceivers, EventType};

// #[axum::debug_handler]
pub(crate) async fn real_time_data(
    ws: WebSocketUpgrade,
    State(events): State<EventReceivers>,
    State(worlds): State<Arc<WorldHelper>>,
) -> Response {
    info!("Handling websocket");
    ws.on_upgrade(move |websocket| async move {
        info!("Upgrading websocket");
        if let Err(e) = handle_socket(websocket, events, worlds).await {
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
) -> Result<(), Box<dyn Error>> {
    let EventReceivers {
        retainers: _,
        listings,
        alerts: _,
        retainer_undercut: _,
        history,
    } = events;
    let (mut sender, mut receiver) = socket.split();
    let mut subscriptions = SelectAll::<BoxStream<ServerClient>>::new();
    sender
        .send(Message::Text(serde_json::to_string(
            &ServerClient::SocketConnected,
        )?))
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
                    return Ok(());
                };
            }
            Either::Right((Some(right), _l)) => {
                info!("Sending websocket message {right:?}");
                sender
                    .send(Message::Text(serde_json::to_string(&right)?))
                    .await?;
            }
            Either::Left((_left, _l)) => {
                // info!("Received none: {left:?}");
                break;
            }
            Either::Right((_right, _r)) => {
                // info!("Right {right:?}");
            }
        };
    }

    Ok(())
}
