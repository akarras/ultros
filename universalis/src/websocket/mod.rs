pub mod event_types;

use crate::WorldId;
use crate::websocket::event_types::{
    Channel, EventChannel, SubscribeMode, WSMessage, WebSocketSubscriptionUpdate, WorldFilter,
};
use async_tungstenite::tokio::{ConnectStream, connect_async};
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
use tokio::sync::mpsc::{Receiver, Sender, channel};

/// Internal SocketTx. Enables the user to communicate with the worker task.
#[derive(Debug)]
enum SocketTx {
    Subscription(WebSocketSubscriptionUpdate),
    Ping,
}

#[derive(Debug)]
pub enum SocketRx {
    Event(Result<WSMessage, crate::Error>),
}

/// Websocket Client for Universalis's real time event API.
/// Handles reconnecting and resubscribing to events on connection loss automatically.
///
/// See the websocket example for an example on how to use.
///
/// Internally, this worker will spawn a task that then uses channels to communicate with the external user,
/// ensuring that the websocket is always read from.
///
pub struct WebsocketClient {
    socket_sender: Sender<SocketTx>,
    listing_receiver: Receiver<SocketRx>,
}

impl WebsocketClient {
    /// Updates subscriptions to data from universalis. Necessary to receieve any data from the API.
    ///
    /// ###Arguments:
    /// * `subscribe_mode` - Whether to to subscribe or unsubscribe. See [SubscribeMode](SubscribeMode) for options
    /// * `channel` - Datatype that you wish to subscribe with See [EventChannel](EventChannel) for options
    /// * `world_id` - Optional [WorldId](World ID), used if you wish to only receive data from a certain world. If None, you will receive data from all worlds.
    ///
    /// ###Example:
    /// ```
    /// use universalis::{WebsocketClient, websocket::event_types::{SubscribeMode, EventChannel}, WorldId};
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let socket_client = WebsocketClient::connect().await;
    ///     socket_client.update_subscription(SubscribeMode::Subscribe, EventChannel::SalesAdd, None).await;
    ///
    /// }
    /// ```
    pub async fn update_subscription(
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
    /// to be used when the socket is reconnected, will resend all the subscriptions previously sent by the user
    async fn resend_subscriptions(
        &self,
        sender: &mut WebSocketStream<ConnectStream>,
    ) -> Result<(), crate::Error> {
        if self.subscriptions.is_empty() {
            warn!("No subscriptions to resend, websocket won't get any data.");
        }
        for channel in &self.subscriptions {
            let subscription_update = WebSocketSubscriptionUpdate {
                event: SubscribeMode::Subscribe,
                channel: channel.clone(),
            };
            let bson = bson::to_vec(&subscription_update)?;
            info!("Resent subscription update {subscription_update:?}");
            sender.send(Message::Binary(bson)).await?;
        }
        Ok(())
    }

    /// track another subscription
    fn subscribe(&mut self, channel: Channel) {
        self.subscriptions.insert(channel);
    }

    /// remove a subscription from the tracker
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
                    .expect("Unable to push message to message queue");
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });
        tokio::spawn(async move {
            let mut active_subscriptions = SubscriptionTracker {
                subscriptions: HashSet::new(),
            };
            loop {
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
                    let cooldown_seconds = std::env::var("UNIVERSALIS_WEBSOCKET_COOLDOWN_SECS")
                        .ok()
                        .and_then(|i| i.parse::<u64>().ok())
                        .unwrap_or(2);
                    warn!("Socket terminated, waiting {cooldown_seconds} seconds and retrying.");
                    tokio::time::sleep(Duration::from_secs(cooldown_seconds)).await;
                    websocket = Self::start_websocket()
                        .await
                        .map_err(|e| error!("Error restarting socket? {e:?}"))
                        .ok();
                    if let Some(mut ws) = websocket {
                        // send a ping first
                        if let Err(ping_result) = ws.send(Message::Ping(vec![1, 2, 3, 4, 5])).await
                        {
                            error!("Error writing ping {ping_result}");
                        }

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
                                if let Err(e) = websocket.send(Message::Binary(bson)).await {
                                    error!("Error sending websocket message {e:?}");
                                }
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
                                if let Err(e) =
                                    websocket.send(Message::Ping(vec![1, 2, 3, 4])).await
                                {
                                    error!("WS Ping Send Error {e:?}");
                                    if let Err(e) = websocket.close(None).await {
                                        error!("Error closing websocket {e:?}");
                                    }
                                }
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
                                    if let Err(e) = sender.send(SocketRx::Event(b)).await {
                                        error!("Error sending websocket data {e:?}");
                                    }
                                });
                            }
                            Message::Ping(p) => {
                                info!("responding to ping with payload: {p:?}");
                                if let Err(e) = websocket.send(Message::Pong(p.clone())).await {
                                    error!("Error sending ping! {e:?}");
                                }
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
                        warn!("Web socket closed");
                    }
                    Either::Right((Some(Err(e)), _)) => {
                        error!("Socket error. Closing socket {e:?}");
                        let socket_close = websocket.close(None).await;
                        info!("closed socket {socket_close:?}");
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
