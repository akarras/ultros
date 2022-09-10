pub mod event_types;

use crate::websocket::event_types::{
    Channel, EventChannel, EventResponse, SubscribeMode, WSMessage, WebSocketSubscriptionUpdate,
    WorldFilter,
};
use crate::WorldId;
use async_tungstenite::tokio::connect_async;
use async_tungstenite::tungstenite::Message;

use bson::{Bson, Document};
use futures::future::Either;

use futures::{SinkExt, Stream, StreamExt};
use log::{debug, error, info, warn};

use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc::{channel, Receiver, Sender};

#[derive(Debug)]
enum SocketTx {
    Subscription(WebSocketSubscriptionUpdate),
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

impl WebsocketClient {
    pub fn get_receiver(&mut self) -> &mut Receiver<SocketRx> {
        &mut self.listing_receiver
    }

    pub async fn connect() -> Self {
        let (mut websocket, response) =
            connect_async("wss://universalis.app/api/ws").await.unwrap();
        info!("Connected Websocket. {} status", response.status());
        info!("Headers: ");
        for (ref header, _value) in response.headers() {
            info!("* {}", header);
        }
        let (socket_sender, mut socket_receiver) = channel(100);
        let (listing_sender, listing_receiver) = channel(100);
        tokio::spawn(async move {
            websocket
                .send(Message::Ping(vec![1, 2, 3, 4]))
                .await
                .unwrap();
            loop {
                match futures::future::select(
                    Box::pin(socket_receiver.recv()),
                    Box::pin(websocket.next()),
                )
                .await
                {
                    Either::Left((sock, _pin)) => match &sock {
                        Some(data) => match data {
                            SocketTx::Subscription(s) => {
                                info!("Subscription update {s:?}");
                                let bson = bson::to_vec(&s).unwrap();
                                websocket.send(Message::Binary(bson)).await.unwrap();
                            }
                        },
                        None => {
                            if let Err(e) = websocket.close(None).await {
                                error!("Unexpected error closing socket {e:?}");
                            }
                            break;
                        }
                    },
                    Either::Right((Some(Ok(message)), _)) => match &message {
                        Message::Text(t) => {
                            info!(
                                "Received text {t}, unexpected only BSON messages were expected."
                            );
                        }
                        Message::Binary(b) => {
                            let b = bson::from_slice::<WSMessage>(b).map_err(|e| {
                                if let Ok(document) = bson::from_slice::<Document>(b) {
                                    error!("valid bson document but not valid struct {document:?}");
                                }
                                e.into()
                            });
                            listing_sender.send(SocketRx::Event(b)).await.unwrap();
                        }
                        Message::Ping(p) => {
                            info!("responding to pong with payload: {p:?}");
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
                    },
                    _ => {
                        debug!("empty stream");
                    }
                }
            }
        });

        Self {
            socket_sender,
            listing_receiver,
        }
    }
}

impl Stream for WebsocketClient {
    type Item = SocketRx;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.listing_receiver.poll_recv(cx)
    }
}
