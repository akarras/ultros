use super::oauth::AuthDiscordUser;
use crate::{
    alerts::undercut_alert::{UndercutResult, UndercutRetainer, UndercutTracker},
    event::EventReceivers,
    utils,
    world_cache::AnySelector,
};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use futures::future::select;
use serde::{Deserialize, Serialize};
use tracing::{
    instrument,
    log::{debug, error, info},
};
use ultros_db::UltrosDb;

#[derive(Debug, Deserialize)]
pub(crate) enum AlertsRx {
    Undercuts {
        margin: i32,
    },
    CreatePriceAlert {
        item_id: i32,
        travel_amount: AnySelector,
        price_threshold: i32,
    },
    Ping(Vec<u8>),
}

#[derive(Debug, Serialize)]
pub(crate) enum AlertsTx {
    RetainerUndercut {
        item_id: i32,
        item_name: String,
        /// List of all the retainers that were just undercut
        undercut_retainers: Vec<UndercutRetainer>,
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
#[instrument]
pub(crate) async fn connect_websocket(
    State(receivers): State<EventReceivers>,
    State(ultros_db): State<UltrosDb>,
    user: AuthDiscordUser,
    websocket: WebSocketUpgrade,
) -> Response {
    info!("creating websocket");
    websocket.on_upgrade(|socket| handle_upgrade(socket, user, receivers, ultros_db))
}

#[instrument(skip(ws))]
async fn handle_upgrade(
    mut ws: WebSocket,
    user: AuthDiscordUser,
    mut receivers: EventReceivers,
    ultros_db: UltrosDb,
) {
    info!("websocket upgraded");
    let mut undercut_tracker: Option<UndercutTracker> = None;
    enum Action {
        Tx(AlertsTx),
        Pong(Vec<u8>),
    }
    loop {
        let result = match select(Box::pin(ws.recv()), Box::pin(receivers.listings.recv())).await {
            futures::future::Either::Left((websocket, _)) => {
                if let Some(received_message) = websocket {
                    let alert_value = match received_message {
                        Ok(message) => match message {
                            Message::Text(message) => match serde_json::from_str(&message) {
                                Ok(ok) => ok,
                                Err(e) => {
                                    error!("{e:?}");
                                    continue;
                                }
                            },
                            Message::Binary(binary) => {
                                match serde_json::from_slice::<AlertsRx>(&binary) {
                                    Ok(ok) => ok,
                                    Err(e) => {
                                        error!("{e:?}");
                                        continue;
                                    }
                                }
                            }
                            Message::Ping(ping) => {
                                // todo figure out how to ping
                                debug!("received ping {ping:?}");
                                AlertsRx::Ping(ping)
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
                    match alert_value {
                        AlertsRx::Undercuts { margin } => {
                            info!("creating undercut tracker");
                            undercut_tracker =
                                UndercutTracker::new(user.id, &ultros_db, margin).await.ok();
                            continue;
                        }
                        AlertsRx::CreatePriceAlert {
                            item_id,
                            travel_amount,
                            price_threshold,
                        } => {
                            info!("price alert tried create, but not implemented");
                            continue;
                        }
                        AlertsRx::Ping(ping) => Action::Pong(ping),
                    }
                } else {
                    // stream has been closed
                    debug!("socket closed");
                    break;
                }
            }
            futures::future::Either::Right((listing_event, _)) => {
                if let Some(undercut) = &mut undercut_tracker {
                    match undercut
                        .handle_listing_event(listing_event.map_err(|e| e.into()))
                        .await
                    {
                        Ok(ok) => match ok {
                            UndercutResult::None => {
                                continue;
                            }
                            UndercutResult::Undercut {
                                item_id,
                                undercut_retainers,
                            } => {
                                let item_name = utils::get_item_name(item_id).to_string();
                                Action::Tx(AlertsTx::RetainerUndercut {
                                    item_id,
                                    item_name,
                                    undercut_retainers,
                                })
                            }
                        },
                        Err(e) => {
                            error!("{e:?}");
                            continue;
                        }
                    }
                } else {
                    continue;
                }
            }
        };

        if let Err(value) = match result {
            Action::Tx(tx) => ws.send(Message::Text(serde_json::to_string(&tx).unwrap())),
            Action::Pong(pong) => ws.send(Message::Pong(pong)),
        }
        .await
        {
            error!("Error sending from {value:?}");
        }
    }
}
