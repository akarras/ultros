pub mod event_types;

use crate::websocket::event_types::{
    Channel, EventChannel, SubscribeMode, WSMessage, WebSocketSubscriptionUpdate, WorldFilter,
};
use crate::WorldId;
use async_tungstenite::tokio::{connect_async, ConnectStream};
use async_tungstenite::tungstenite::Message;

use bson::Document;
use futures::future::Either;

use futures::{SinkExt, Stream, StreamExt};
use log::{error, info, warn};

use async_tungstenite::WebSocketStream;
use futures::stream::FusedStream;
use std::collections::HashSet;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::mpsc::{channel, Receiver, Sender};

#[derive(Debug)]
enum SocketTx {
    Subscription(WebSocketSubscriptionUpdate),
    Ping,
}

#[derive(Debug)]
pub enum SocketRx {
    Event(Result<WSMessage, crate::Error>),
}

pub struct WebsocketClient {
    socket_sender: Sender<SocketTx>,
    listing_receiver: Receiver<SocketRx>,
}

impl WebsocketClient {
    /// Creates a websocket subscription
    pub async fn subscribe(
        &self,
        subscribe_mode: SubscribeMode,
        channel: EventChannel,
        world_id: Option<WorldId>,
    ) {
        self.socket_sender
            .send(SocketTx::Subscription(WebSocketSubscriptionUpdate::new(
                subscribe_mode,
                Channel::new(channel, world_id.map(WorldFilter::new)),
            )))
            .await
            .unwrap();
    }
}

/// Internally keeps track of the state of what subscriptions have been sent
struct SubscriptionTracker {
    subscriptions: HashSet<Channel>,
}

impl SubscriptionTracker {
    async fn resend_subscriptions(
        &self,
        sender: &mut WebSocketStream<ConnectStream>,
    ) -> Result<(), crate::Error> {
        for channel in &self.subscriptions {
            let bson = bson::to_vec(&WebSocketSubscriptionUpdate {
                event: SubscribeMode::Subscribe,
                channel: channel.clone(),
            })?;

            sender.send(Message::Binary(bson)).await?;
        }
        Ok(())
    }

    fn subscribe(&mut self, channel: Channel) {
        self.subscriptions.insert(channel);
    }

    fn unsubscribe(&mut self, channel: &Channel) {
        self.subscriptions.remove(channel);
    }
}

impl WebsocketClient {
    pub fn get_receiver(&mut self) -> &mut Receiver<SocketRx> {
        &mut self.listing_receiver
    }

    pub async fn connect() -> Self {
        let mut websocket: Option<WebSocketStream<ConnectStream>> = Self::start_websocket()
            .await
            .map_err(|e| error!("{e:?}"))
            .ok();
        let (socket_sender, mut socket_receiver) = channel(100);
        let (listing_sender, listing_receiver) = channel(100);
        let sender = socket_sender.clone();
        tokio::spawn(async move {
            loop {
                info!("Sending ping to keep the socket alive");
                sender
                    .send(SocketTx::Ping)
                    .await
                    .expect("local sender failed to send ping");
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });
        tokio::spawn(async move {
            loop {
                let mut active_subscriptions = SubscriptionTracker {
                    subscriptions: HashSet::new(),
                };
                if let Some(ws) = websocket {
                    if ws.is_terminated() {
                        websocket = None;
                        warn!("websocket terminated, restarting");
                        continue;
                    } else {
                        websocket = Some(ws);
                    }
                }
                let websocket = if let Some(websocket) = &mut websocket {
                    websocket
                } else {
                    warn!("Socket terminated, waiting 30 seconds and retrying.");
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    websocket = Self::start_websocket()
                        .await
                        .map_err(|e| error!("{e:?}"))
                        .ok();
                    if let Some(mut ws) = websocket {
                        if let Err(e) = active_subscriptions.resend_subscriptions(&mut ws).await {
                            error!("error resending subscriptions {e:?}");
                            websocket = None;
                        } else {
                            websocket = Some(ws);
                        }
                    }
                    continue;
                };
                match futures::future::select(
                    Box::pin(socket_receiver.recv()),
                    Box::pin(websocket.next()),
                )
                .await
                {
                    Either::Left((sock, _pin)) => match sock {
                        Some(data) => match data {
                            SocketTx::Subscription(s) => {
                                info!("Subscription update {s:?}");
                                let bson = bson::to_vec(&s).unwrap();
                                websocket.send(Message::Binary(bson)).await.unwrap();
                                // keep track of the subscriptions so if the socket closes we can update accordingly
                                let WebSocketSubscriptionUpdate { event, channel } = s;
                                match event {
                                    SubscribeMode::Subscribe => {
                                        active_subscriptions.subscribe(channel)
                                    }
                                    SubscribeMode::Unsubscribe => {
                                        active_subscriptions.unsubscribe(&channel)
                                    }
                                }
                            }
                            SocketTx::Ping => {
                                websocket
                                    .send(Message::Ping(vec![1, 2, 3, 4]))
                                    .await
                                    .unwrap();
                            }
                        },
                        None => {
                            if let Err(e) = websocket.close(None).await {
                                error!("Unexpected error closing socket {e:?}");
                            }
                            break;
                        }
                    },
                    Either::Right((Some(Ok(message)), _)) => {
                        match message {
                            Message::Text(t) => {
                                info!(
                                "Received text {t}, unexpected only BSON messages were expected."
                            );
                            }
                            Message::Binary(b) => {
                                let sender = listing_sender.clone();
                                tokio::spawn(async move {
                                    let b = bson::from_slice::<WSMessage>(&b).map_err(|e| {
                                    if let Ok(document) = bson::from_slice::<Document>(&b) {
                                        error!("valid bson document but not valid struct {document:?}");
                                    }
                                    e.into()
                                });
                                    sender.send(SocketRx::Event(b)).await.unwrap();
                                });
                            }
                            Message::Ping(p) => {
                                info!("responding to ping with payload: {p:?}");
                                websocket.send(Message::Pong(p.clone())).await.unwrap();
                            }
                            Message::Pong(pong) => {
                                info!("got pong! {pong:?}");
                            }
                            Message::Close(closed) => {
                                info!("Socket closed with reason {closed:?}");
                            }
                            Message::Frame(frame) => {
                                info!("received frame: {frame:?}");
                            }
                        }
                    }
                    Either::Right((None, _)) => {
                        info!("Web socket closed");
                    }
                    Either::Right((Some(Err(e)), _)) => {
                        error!("Socket error. Closing socket {e:?}");
                        let socket = websocket.close(None).await;
                    }
                }
            }
        });

        Self {
            socket_sender,
            listing_receiver,
        }
    }

    async fn start_websocket() -> Result<WebSocketStream<ConnectStream>, crate::Error> {
        let (websocket, response) = connect_async("wss://universalis.app/api/ws").await?;
        info!("Connected Websocket. {} status", response.status());
        info!("Headers: ");
        for (ref header, _value) in response.headers() {
            info!("* {}", header);
        }
        Ok(websocket)
    }
}

impl Stream for WebsocketClient {
    type Item = SocketRx;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.listing_receiver.poll_recv(cx)
    }
}
