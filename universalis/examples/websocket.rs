use futures::StreamExt;
use log::debug;
use universalis::websocket::event_types::{EventChannel, SubscribeMode};
use universalis::{UniversalisClient, WebsocketClient};

#[tokio::main]
async fn main() {
    // subscribe to several items
    tracing_subscriber::fmt::init();
    let universalis_client = UniversalisClient::new();
    let worlds = universalis_client.get_worlds().await.unwrap();
    let sargatanas = worlds.0.iter().find(|w| w.name.0 == "Sargatanas").unwrap();
    let mut ws = WebsocketClient::connect().await;
    ws.subscribe(
        SubscribeMode::Subscribe,
        EventChannel::ListingsAdd,
        Some(sargatanas.id),
    )
    .await;
    ws.subscribe(
        SubscribeMode::Subscribe,
        EventChannel::ListingsRemove,
        Some(sargatanas.id),
    )
    .await;
    ws.subscribe(
        SubscribeMode::Subscribe,
        EventChannel::SalesAdd,
        Some(sargatanas.id),
    )
    .await;
    ws.subscribe(
        SubscribeMode::Subscribe,
        EventChannel::SalesRemove,
        Some(sargatanas.id),
    )
    .await;
    let receiver = ws.get_receiver();
    loop {
        let next = receiver.recv().await;
        debug!("{next:?}");
    }
}
