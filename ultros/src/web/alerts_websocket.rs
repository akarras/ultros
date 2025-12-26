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
    enum Action {
        Tx(AlertsTx),
        Pong(Vec<u8>),
    }
    let (action_tx, mut action_rx) = tokio::sync::mpsc::channel::<Action>(10);
    loop {
        let result = match select(
            Box::pin(ws.recv()),
            select(
                Box::pin(receivers.listings.recv()),
                Box::pin(action_rx.recv()),
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
                        AlertsRx::CreatePriceAlert {
                            item_id,
                            travel_amount,
                            price_threshold,
                        } => {
                            let travel_amount = match travel_amount {
                                ultros_api_types::world_helper::AnySelector::World(w) => {
                                    ultros_db::world_cache::AnySelector::World(w)
                                }
                                ultros_api_types::world_helper::AnySelector::Datacenter(d) => {
                                    ultros_db::world_cache::AnySelector::Datacenter(d)
                                }
                                ultros_api_types::world_helper::AnySelector::Region(r) => {
                                    ultros_db::world_cache::AnySelector::Region(r)
                                }
                            };
                            let mut alert = price_alert_service
                                .create_alert(price_threshold, item_id, travel_amount)
                                .await;
                            let action_tx = action_tx.clone();
                            tokio::spawn(async move {
                                while let Some(PriceUndercutData {
                                    item_id,
                                    undercut_by,
                                    world_id,
                                }) = alert.recv().await
                                {
                                    let item_name = utils::get_item_name(item_id).to_string();
                                    if let Err(e) = action_tx
                                        .send(Action::Tx(AlertsTx::PriceAlert {
                                            item_id,
                                            item_name,
                                            price: undercut_by,
                                            world_id,
                                        }))
                                        .await
                                    {
                                        error!("Error sending alert {e:?}");
                                        break;
                                    }
                                }
                            });
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
            futures::future::Either::Right((either_listing_or_action, _)) => {
                match either_listing_or_action {
                    futures::future::Either::Left((listing_event, _)) => {
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
                                            undercut_retainers: undercut_retainers.into_iter().map(|r| ultros_api_types::websocket::UndercutRetainer {
                                                id: r.id,
                                                name: r.name,
                                                undercut_amount: r.undercut_amount,
                                            }).collect(),
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
                    futures::future::Either::Right((action, _)) => {
                        if let Some(action) = action {
                            action
                        } else {
                            break;
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
