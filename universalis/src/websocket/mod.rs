pub mod event_types;

use crate::websocket::event_types::{
    Channel, EventChannel, EventResponse, SubscribeMode, WebSocketSubscriptionUpdate, WorldFilter,
};
use crate::{ListingView, WorldId};
use async_tungstenite::tokio::{connect_async, ConnectStream};
use async_tungstenite::tungstenite::Message;
use async_tungstenite::WebSocketStream;
use bson::Bson;
use futures::future::{Either, Select};
use futures::stream::{FusedStream, Next};
use futures::{SinkExt, Stream, StreamExt, TryStreamExt};
use log::{debug, info, warn};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc::{channel, Receiver, Sender};

#[derive(Debug)]
enum SocketTx {
    Subscription(WebSocketSubscriptionUpdate),
    Close,
}

#[derive(Debug)]
pub enum SocketRx {
    Event(EventResponse),
}

pub struct WebsocketClient {
    channels: Vec<Channel>,
    socket_sender: Sender<SocketTx>,
    listing_receiver: Receiver<SocketRx>,
}

impl WebsocketClient {
    pub async fn subscribe(
        &self,
        subscribe_mode: SubscribeMode,
        channel: EventChannel,
        world_id: Option<WorldId>,
    ) {
        self.socket_sender
            .send(SocketTx::Subscription(WebSocketSubscriptionUpdate::new(
                subscribe_mode,
                Channel::new(channel, world_id.map(|m| WorldFilter::new(m))),
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
                    Either::Left((Some(data), _)) => match &data {
                        SocketTx::Subscription(s) => {
                            info!("Subscription update {s:?}");
                            let bson = bson::to_vec(&s).unwrap();
                            websocket.send(Message::Binary(bson)).await.unwrap();
                        }
                        SocketTx::Close => {
                            break;
                            info!("Closing socket?");
                        }
                    },
                    Either::Right((Some(Ok(message)), _)) => {
                        match &message {
                            Message::Text(t) => {
                                info!("Received text {t}");
                            }
                            Message::Binary(b) => {
                                if let Ok(b) = bson::from_slice::<EventResponse>(b) {
                                    listing_sender.send(SocketRx::Event(b)).await.unwrap();
                                } else {
                                    let b: Bson = bson::from_slice(&b).unwrap();
                                    warn!("Received invalid bson data {b:?}");
                                }
                            }
                            Message::Ping(p) => {
                                info!("responding to pong with payload: {p:?}");
                                // websocket.send(Message::Pong(p)).await.unwrap();
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
                    _ => {
                        debug!("empty stream");
                    }
                }
            }
        });

        Self {
            channels: vec![],
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
