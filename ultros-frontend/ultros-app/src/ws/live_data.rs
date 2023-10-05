use std::collections::VecDeque;

use crate::components::live_sale_ticker::SaleView;
use crate::error::AppError;
use futures::{SinkExt, StreamExt};
use gloo_net::websocket::{futures::WebSocket, Message};
use leptos::{RwSignal, SignalUpdate};
use log::error;
use ultros_api_types::{
    websocket::{ClientMessage, EventType, FilterPredicate, ServerClient, SocketMessageType},
    world_helper::AnySelector,
};

pub(crate) async fn live_sales(
    signal: RwSignal<VecDeque<SaleView>>,
    price_zone: AnySelector,
) -> Result<(), AppError> {
    use log::info;

    log::info!("CONNECTING TO SALES!");
    // TODO - better way to switch to wss
    #[cfg(debug_assertions)]
    let url = "ws://localhost:8080/api/v1/realtime/events";
    #[cfg(not(debug_assertions))]
    let url = "wss://ultros.app/api/v1/realtime/events";
    let socket = WebSocket::open(url).unwrap();
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
                                            for (sale, _) in add.sales {
                                                sales.push_front(SaleView {
                                                    item_id: sale.sold_item_id,
                                                    price: sale.price_per_item,
                                                    sold_date: sale.sold_date,
                                                });
                                            }
                                            sales.make_contiguous().sort_by_key(|sale| {
                                                std::cmp::Reverse(sale.sold_date)
                                            });
                                            while sales.len() > 8 {
                                                sales.pop_back();
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
