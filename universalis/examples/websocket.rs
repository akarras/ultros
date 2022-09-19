use log::{debug, info};
use std::borrow::Borrow;
use std::time::Instant;
use universalis::websocket::event_types::{EventChannel, SubscribeMode, WSMessage};
use universalis::{UniversalisClient, WebsocketClient};

#[tokio::main]
async fn main() {
    // subscribe to several items
    tracing_subscriber::fmt::init();
    let universalis_client = UniversalisClient::new();
    let worlds = universalis_client.get_worlds().await.unwrap();
    let sargatanas = worlds.0.iter().find(|w| w.name.0 == "Sargatanas").unwrap();
    let mut ws = WebsocketClient::connect().await;
    ws.update_subscription(
        SubscribeMode::Subscribe,
        EventChannel::ListingsAdd,
        Some(sargatanas.id),
    )
    .await;
    // ws.subscribe(
    //     SubscribeMode::Subscribe,
    //     EventChannel::ListingsRemove,
    //     Some(sargatanas.id),
    // )
    // .await;
    // ws.subscribe(
    //     SubscribeMode::Subscribe,
    //     EventChannel::SalesAdd,
    //     Some(sargatanas.id),
    // )
    // .await;
    // ws.subscribe(
    //     SubscribeMode::Subscribe,
    //     EventChannel::SalesRemove,
    //     Some(sargatanas.id),
    // )
    // .await;
    // let receiver = ws.get_receiver();
    let mut last_message_received = Instant::now();
    loop {
        if let Some(next) = ws {
            if let universalis::websocket::SocketRx::Event(Ok(WSMessage::ListingsAdd {
                item,
                world,
                listings,
            })) = next
            {
                if item.0 == 27842 {
                    for listing in listings {
                        info!("{listing:?}");
                    }
                }
            }
            // print one example of each event, so lets unsubscribe from the channel
            /*ws.subscribe(SubscribeMode::Unsubscribe, match next {
                universalis::websocket::SocketRx::Event(Ok(msg)) => {
                    EventChannel::from(&msg)
                },
                _ => {panic!("unexpected error")}
            }, Some(sargatanas.id)).await;*/
        } else {
            break;
        }
    }
}
