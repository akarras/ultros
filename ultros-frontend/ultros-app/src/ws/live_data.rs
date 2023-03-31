use std::collections::VecDeque;

use futures::{SinkExt, StreamExt};
use gloo_net::websocket::{futures::WebSocket, Message};
use leptos::{RwSignal, SignalUpdate};
use log::error;
use ultros_api_types::{
    websocket::{ClientMessage, EventType, FilterPredicate, ServerClient, SocketMessageType},
    world_helper::AnySelector,
    SaleHistory, UnknownCharacter,
};

use crate::error::AppError;

pub(crate) async fn live_sales(
    signal: RwSignal<VecDeque<(SaleHistory, UnknownCharacter)>>,
    price_zone: AnySelector,
) -> Result<(), AppError> {
    use log::info;

    log::info!("CONNECTING TO SALES!");
    let socket = WebSocket::open("ws://localhost:8080/api/v1/realtime/events").unwrap();
    let (mut write, mut read) = socket.split();
    let client = ClientMessage::AddSubscribe {
        filter: FilterPredicate::World(price_zone),
        msg_type: SocketMessageType::Sales,
    };
    write
        .send(Message::Text(serde_json::to_string(&client).unwrap()))
        .await
        .unwrap();
    while let Some(msg) = read.next().await {
        match msg {
            Ok(o) => match o {
                Message::Text(o) => {
                    if let Ok(val) = serde_json::from_str::<ServerClient>(&o) {
                        info!("{val:?}");
                        match val {
                            ServerClient::Sales(sig) => match sig {
                                EventType::Added(add) => {
                                    if signal
                                        .try_update(|sales| {
                                            for sale in add.sales {
                                                sales.push_back(sale);
                                            }
                                            while sales.len() > 10 {
                                                sales.pop_front();
                                            }
                                        })
                                        .is_none()
                                    {
                                        info!("Socket closed");
                                        return Ok(());
                                    }
                                }
                                _ => {}
                            },
                            ServerClient::Listings(l) => log::info!("Listings {l:?}"),
                            ServerClient::SubscriptionCreated => {
                                log::info!("Subscription created");
                            }
                            ServerClient::SocketConnected => {
                                log::info!("Socket connected");
                            }
                        }
                    }
                }
                Message::Bytes(_) => {
                    error!("Received bytes?");
                }
            },
            Err(e) => {
                error!("Websocket error {e:?}")
            }
        }
    }
    Ok(())
}
