pub mod event_types;

use crate::WorldId;
use crate::websocket::event_types::{
    Channel, EventChannel, SubscribeMode, WSMessage, WebSocketSubscriptionUpdate, WorldFilter,
};
use async_tungstenite::tokio::{ConnectStream, connect_async};
use async_tungstenite::tungstenite::Message;
use async_tungstenite::tungstenite::client::IntoClientRequest;
use async_tungstenite::tungstenite::http::HeaderValue;
use async_tungstenite::tungstenite::http::header::USER_AGENT;

use bson::Document;
use futures::future::Either;

use futures::{Stream, StreamExt};
use tracing::{error, info, warn};

use async_tungstenite::WebSocketStream;
use futures::stream::FusedStream;
use std::collections::HashSet;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{Receiver, Sender, channel};

/// How often we send a websocket Ping to keep the connection alive.
const PING_INTERVAL: Duration = Duration::from_secs(60);

/// How long the socket may go without delivering *any* frame — Pong, data, or
/// otherwise — before we treat it as dead and reconnect.
///
/// Universalis' server replies to our Ping, so silence this long means the
/// connection is gone. The failure mode this guards against is a half-open TCP
/// connection: the peer vanished without a FIN/RST, so `websocket.next()` never
/// yields, never errors, and `is_terminated()` stays false. The socket sits
/// there looking connected while delivering zero market data, forever, and the
/// existing reconnect path is never reached because nothing ever signals a
/// close. Without this deadline the only fix is a process restart.
///
/// 2.5× [`PING_INTERVAL`], so one slow round-trip is tolerated and two
/// consecutive missed Pongs trip it. Override with
/// `UNIVERSALIS_WEBSOCKET_LIVENESS_TIMEOUT_SECS`.
const DEFAULT_LIVENESS_TIMEOUT: Duration = Duration::from_secs(150);

fn liveness_timeout() -> Duration {
    std::env::var("UNIVERSALIS_WEBSOCKET_LIVENESS_TIMEOUT_SECS")
        .ok()
        .and_then(|i| i.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_LIVENESS_TIMEOUT)
}

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
    ///
    /// (`no_run`: executing this would dial the live Universalis websocket
    /// from `cargo test` — and it panics in the doctest harness anyway, since
    /// the workspace enables both rustls backends and only real binaries
    /// install a process-level CryptoProvider first.)
    /// ```no_run
    /// use universalis::{WebsocketClient, websocket::event_types::{SubscribeMode, EventChannel}, WorldId};
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let socket_client = WebsocketClient::connect("my-app/1.0 (contact@example.com)").await;
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
            let bson = bson::serialize_to_vec(&subscription_update)?;
            info!("Resent subscription update {subscription_update:?}");
            sender.send(Message::Binary(bson.into())).await?;
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

    pub async fn connect(user_agent: impl Into<String>) -> Self {
        let user_agent: String = user_agent.into();
        let mut websocket: Option<WebSocketStream<ConnectStream>> =
            Self::start_websocket(&user_agent)
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
                tokio::time::sleep(PING_INTERVAL).await;
            }
        });
        tokio::spawn(async move {
            let mut active_subscriptions = SubscriptionTracker {
                subscriptions: HashSet::new(),
            };
            let liveness_timeout = liveness_timeout();
            // Last time the socket handed us anything at all. The ping task
            // wakes this loop every PING_INTERVAL even when the socket is
            // silent, so the check below runs on a fixed cadence.
            let mut last_frame_at = Instant::now();
            loop {
                if let Some(ws) = websocket {
                    if ws.is_terminated() {
                        websocket = None;
                        warn!("websocket terminated, restarting");
                        continue;
                    } else if last_frame_at.elapsed() > liveness_timeout {
                        warn!(
                            silent_for_secs = last_frame_at.elapsed().as_secs(),
                            timeout_secs = liveness_timeout.as_secs(),
                            "websocket delivered no frames within the liveness deadline, \
                             assuming it is half-open and reconnecting"
                        );
                        metrics::counter!("ultros_websocket_liveness_timeouts_total").increment(1);
                        // Deliberately NOT `ws.close(None).await` here: close()
                        // sends a Close frame and awaits the flush, and against
                        // a half-open peer that write sits in the kernel's
                        // retransmit queue for tcp_retries2 (~13-30 minutes) —
                        // stalling this single worker loop, which is the exact
                        // silent stall the liveness deadline exists to break.
                        // Dropping the stream closes the fd immediately without
                        // waiting on the vanished peer.
                        drop(ws);
                        websocket = None;
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
                    websocket = Self::start_websocket(&user_agent)
                        .await
                        .map_err(|e| error!("Error restarting socket? {e:?}"))
                        .ok();
                    if let Some(mut ws) = websocket {
                        // send a ping first
                        if let Err(ping_result) =
                            ws.send(Message::Ping(vec![1, 2, 3, 4, 5].into())).await
                        {
                            error!("Error writing ping {ping_result}");
                        }

                        if let Err(e) = active_subscriptions.resend_subscriptions(&mut ws).await {
                            error!("error resending subscriptions {e:?}");
                            websocket = None;
                        } else {
                            // Start the deadline from the fresh connection, not
                            // from whenever the dead one last spoke.
                            last_frame_at = Instant::now();
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
                                let bson = bson::serialize_to_vec(&s).unwrap();
                                if let Err(e) = websocket.send(Message::Binary(bson.into())).await {
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
                                    websocket.send(Message::Ping(vec![1, 2, 3, 4].into())).await
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
                        // Any frame proves the socket is alive — a Pong is the
                        // one we expect during a quiet market, but data, Ping
                        // and Close all count too.
                        last_frame_at = Instant::now();
                        match message {
                            Message::Text(t) => {
                                info!(
                                    "Received text {t}, unexpected only BSON messages were expected."
                                );
                            }
                            Message::Binary(b) => {
                                let sender = listing_sender.clone();
                                tokio::spawn(async move {
                                    let b = bson::deserialize_from_slice::<WSMessage>(b.as_ref()).map_err(|e| {
                                    if let Ok(document) = bson::deserialize_from_slice::<Document>(b.as_ref()) {
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

    async fn start_websocket(
        user_agent: &str,
    ) -> Result<WebSocketStream<ConnectStream>, crate::Error> {
        let mut request = "wss://universalis.app/api/ws".into_client_request()?;
        match HeaderValue::from_str(user_agent) {
            Ok(value) => {
                request.headers_mut().insert(USER_AGENT, value);
            }
            Err(e) => warn!("Invalid websocket user-agent {user_agent:?}, connecting without: {e}"),
        }
        let (websocket, response) = connect_async(request).await?;
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
