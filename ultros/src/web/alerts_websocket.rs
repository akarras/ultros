use super::oauth::AuthDiscordUser;
use crate::{
    alerts::undercut_alert::{Undercut, UndercutTracker},
    event::EventReceivers,
    utils,
};
use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
};
use futures::future::select;
use tracing::{
    instrument,
    log::{debug, error, info},
};
use ultros_api_types::websocket::{AlertsRx, AlertsTx};
use ultros_db::UltrosDb;

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
    let mut watched_character: Option<String> = None;

    enum Action {
        Tx(AlertsTx),
        Pong(Vec<u8>),
    }
    loop {
        let result = match select(
            Box::pin(ws.recv()),
            select(
                Box::pin(receivers.listings.recv()),
                Box::pin(receivers.history.recv()),
            ),
        )
        .await
        {
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
                        AlertsRx::WatchCharacter { name } => {
                            info!("watching character {name}");
                            watched_character = Some(name.to_lowercase());
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
            futures::future::Either::Right((
                futures::future::Either::Left((listing_event, _)),
                _,
            )) => {
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
                                    undercut_retainers: undercut_retainers
                                        .into_iter()
                                        .map(|u| ultros_api_types::websocket::UndercutRetainer {
                                            id: u.id,
                                            name: u.name,
                                            undercut_amount: u.undercut_amount,
                                        })
                                        .collect(),
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
            futures::future::Either::Right((
                futures::future::Either::Right((sale_event, _)),
                _,
            )) => {
                if let Some(watched_char) = &watched_character {
                    if let Ok(crate::event::EventType::Add(data)) = sale_event {
                        // find the first matching sale
                        if let Some(sale) = data.sales.iter().find_map(|(sale, character)| {
                            if character.name.to_lowercase() == *watched_char {
                                Some(sale)
                            } else {
                                None
                            }
                        }) {
                            Action::Tx(AlertsTx::ItemPurchased {
                                item_id: sale.sold_item_id,
                            })
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                } else {
                    continue;
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
