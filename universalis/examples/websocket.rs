use clap::Parser;
use log::info;
use universalis::websocket::event_types::{EventChannel, SubscribeMode};
use universalis::websocket::SocketRx;
use universalis::{ItemId, UniversalisClient, WebsocketClient};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[arg(short, long)]
    world_name: Option<String>,
    #[arg(short, long)]
    item_ids: Option<Vec<i32>>,
}

#[tokio::main]
async fn main() {
    // subscribe to several items
    pretty_env_logger::init();
    let universalis_client = UniversalisClient::new("ultros-universalis-examples");
    let worlds = universalis_client.get_worlds().await.unwrap();
    let args = Args::parse();
    let world = args
        .world_name
        .map(|world| {
            worlds
                .0
                .iter()
                .find(|w| w.name.0.eq_ignore_ascii_case(&world))
                .expect("Unable to find a world matching name specfied")
        })
        .map(|w| w.id);
    let mut ws = WebsocketClient::connect().await;
    ws.update_subscription(SubscribeMode::Subscribe, EventChannel::ListingsAdd, world)
        .await;
    ws.update_subscription(
        SubscribeMode::Subscribe,
        EventChannel::ListingsRemove,
        world,
    )
    .await;
    ws.update_subscription(SubscribeMode::Subscribe, EventChannel::SalesAdd, world)
        .await;
    ws.update_subscription(SubscribeMode::Subscribe, EventChannel::SalesRemove, world)
        .await;
    loop {
        if let Some(next) = ws.get_receiver().recv().await {
            match next {
                SocketRx::Event(Ok(e)) => {
                    let item_id = ItemId::from(&e);

                    if args.item_ids.is_some() {
                        if args
                            .item_ids
                            .as_ref()
                            .map(|i| i.contains(&item_id.0))
                            .unwrap_or(true)
                        {
                            info!("Received event {e:?}");
                        }
                    } else {
                        info!("Received event {e:?}");
                    }
                }
                _ => {}
            }
        } else {
            break;
        }
    }
}
