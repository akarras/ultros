use super::oauth::AuthDiscordUser;
use crate::{event::EventReceivers, world_cache::AnySelector};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use serde::{Deserialize, Serialize};
use tracing::log::{debug, error};

#[derive(Debug, Deserialize)]
pub(crate) enum AlertsRx {
    Undercuts,
    CreatePriceAlert {
        item_id: i32,
        travel_amount: AnySelector,
        price_threshold: i32,
    },
}

#[derive(Debug, Serialize)]
pub(crate) enum AlertsTx {
    RetainerUndercut {
        item_id: i32,
        item_name: String,
        /// List of all the retainers that were just undercut
        retainer_names: Vec<String>,
    },
    PriceAlert {
        world_id: i32,
        item_id: i32,
        item_name: String,
        price: i32,
    },
}

/// Websocket connection will enable the user to receive real time events for alerts.
/// The websocket messages are defined by AlertsTx, AlertsRx.
/// The websocket shall also only allow one websocket per user.
/// The websocket will use the authentication cookie to validate the user.
pub(crate) async fn connect_websocket(
    State(receivers): State<EventReceivers>,
    user: AuthDiscordUser,
    websocket: WebSocketUpgrade,
) -> Response {
    websocket.on_upgrade(|socket| handle_upgrade(socket, user, receivers))
}

async fn handle_upgrade(mut ws: WebSocket, user: AuthDiscordUser, receivers: EventReceivers) {
    loop {
        if let Some(received_message) = ws.recv().await {
            let pong = match received_message {
                Ok(message) => match message {
                    Message::Text(message) => match serde_json::from_str(&message) {
                        Ok(ok) => ok,
                        Err(e) => {
                            error!("{e:?}");
                            continue;
                        }
                    },
                    Message::Binary(binary) => match serde_json::from_slice::<AlertsRx>(&binary) {
                        Ok(ok) => ok,
                        Err(e) => {
                            error!("{e:?}");
                            continue;
                        }
                    },
                    Message::Ping(ping) => {
                        debug!("received ping {ping:?}");
                        if let Err(e) = ws.send(Message::Ping(ping)).await {
                            error!("{e:?}");
                        }
                        continue;
                    }
                    Message::Pong(pong) => {
                        debug!("received pong {pong:?}");
                        continue;
                    }
                    Message::Close(close) => {
                        debug!("socket closed {close:?}");
                        break;
                    }
                },
                Err(e) => {
                    error!("{e:?}");
                    break;
                }
            };
        } else {
            // stream has been closed
            debug!("socket closed");
            break;
        }
    }
}
