use std::collections::VecDeque;

use crate::components::live_sale_ticker::SaleView;
use crate::error::AppError;
use futures::{SinkExt, StreamExt};
use gloo_net::websocket::{Message, futures::WebSocket};
use itertools::Itertools;
use leptos::prelude::{RwSignal, Update};
use log::error;
use ultros_api_types::{
    websocket::{ClientMessage, EventType, FilterPredicate, ServerClient, SocketMessageType},
    world_helper::AnySelector,
};

pub(crate) async fn live_sales(
    signal: RwSignal<VecDeque<SaleView>>,
    price_zone: AnySelector,
) -> Result<(), AppError> {
    let window = web_sys::window().unwrap();
    let location = window.location();
    let protocol = location.protocol().unwrap();
    let host = location.host().unwrap();
    let ws_protocol = if protocol == "https:" { "wss" } else { "ws" };
    let url = format!("{}://{}/api/v1/realtime/events", ws_protocol, host);

    let socket = WebSocket::open(&url).unwrap();
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
                                                    hq: sale.hq,
                                                });
                                            }
                                            sales.make_contiguous().sort_by_key(|sale| {
                                                std::cmp::Reverse(sale.sold_date)
                                            });
                                            *sales = sales
                                                .iter()
                                                .unique_by(|sale| (sale.item_id, sale.hq))
                                                .take(8)
                                                .cloned()
                                                .collect();
                                        })
                                        .is_none()
                                    {
                                        return Ok(());
                                    }
                                }
                                _ => {}
                            },
                            ServerClient::Listings(_l) => {}
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
