use super::oauth::AuthDiscordUser;
use crate::{
    alerts::{
        price_alert::{PriceAlertService, PriceUndercutData},
        undercut_alert::{Undercut, UndercutTracker},
    },
    event::EventReceivers,
    utils,
};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use futures::future::select;
use tokio::sync::mpsc;
use tracing::{
    instrument,
    log::{debug, error, info},
};
use ultros_api_types::alerts::{AlertsRx, AlertsTx};
use ultros_db::UltrosDb;

/// Websocket connection will enable the user to receive real time events for alerts.
/// The websocket messages are defined by AlertsTx, AlertsRx.
/// The websocket shall also only allow one websocket per user.
/// The websocket will use the authentication cookie to validate the user.
#[instrument]
pub(crate) async fn connect_websocket(
    State(receivers): State<EventReceivers>,
    State(ultros_db): State<UltrosDb>,
    State(price_alert_service): State<PriceAlertService>,
    user: AuthDiscordUser,
    websocket: WebSocketUpgrade,
) -> Response {
    info!("creating websocket");
    websocket.on_upgrade(|socket| {
        handle_upgrade(socket, user, receivers, ultros_db, price_alert_service)
    })
}

#[instrument(skip(ws))]
async fn handle_upgrade(
    mut ws: WebSocket,
    user: AuthDiscordUser,
    mut receivers: EventReceivers,
    ultros_db: UltrosDb,
    price_alert_service: PriceAlertService,
) {
    info!("websocket upgraded");
    let mut undercut_tracker: Option<UndercutTracker> = None;
    let (price_tx, mut price_rx) = mpsc::channel::<PriceUndercutData>(100);

    enum Action {
        Tx(AlertsTx),
        Pong(Vec<u8>),
    }
    loop {
        // We now select on 3 things:
        // 1. WebSocket incoming messages
        // 2. EventBus listing events (for UndercutTracker)
        // 3. Price Alert channel events

        let ws_recv = Box::pin(ws.recv());
        let listing_recv = Box::pin(receivers.listings.recv());
        let price_recv = Box::pin(price_rx.recv());

        let result = match select(ws_recv, select(listing_recv, price_recv)).await {
            futures::future::Either::Left((websocket, _others)) => {
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
                                debug!("received ping {ping:?}");
                                AlertsRx::Ping(ping.to_vec())
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
                            info!("Creating price alert for {item_id}");
                            price_alert_service
                                .create_alert(
                                    price_threshold,
                                    item_id,
                                    travel_amount,
                                    price_tx.clone(),
                                )
                                .await;
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
            futures::future::Either::Right((inner, _ws_fut)) => {
                match inner {
                    futures::future::Either::Left((listing_event, _price_fut)) => {
                        // Handle listing event for UndercutTracker
                        if let Some(undercut) = &mut undercut_tracker {
                            match undercut
                                .handle_listing_event(listing_event.map_err(|e| e.into()))
                                .await
                            {
                                Ok(ok) => match ok {
                                    None => {
                                        continue;
                                    }
                                    Some(Undercut {
                                        item_id,
                                        undercut_retainers,
                                    }) => {
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
                    futures::future::Either::Right((price_alert, _listing_fut)) => {
                        // Handle price alert
                        if let Some(PriceUndercutData {
                            item_id,
                            price,
                            world_id,
                        }) = price_alert
                        {
                            let item_name = utils::get_item_name(item_id).to_string();
                            Action::Tx(AlertsTx::PriceAlert {
                                world_id,
                                item_id,
                                item_name,
                                price,
                            })
                        } else {
                            // price channel closed? Shouldn't happen unless we close tx.
                            continue;
                        }
                    }
                }
            }
        };

        if let Err(value) = match result {
            Action::Tx(tx) => ws.send(Message::Text(serde_json::to_string(&tx).unwrap().into())),
            Action::Pong(pong) => ws.send(Message::Pong(pong.into())),
        }
        .await
        {
            error!("Error sending from {value:?}");
        }
    }
}
