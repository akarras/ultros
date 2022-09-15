mod discord;
mod web;

use crate::web::WebState;
use anyhow::Result;
use discord::start_discord;
use std::sync::Arc;
use tracing::{error, info};
use ultros_db::UltrosDb;
use universalis::websocket::event_types::{EventChannel, SaleView, SubscribeMode, WSMessage};
use universalis::websocket::SocketRx;
use universalis::{
    DataCentersView, ItemId, UniversalisClient, WebsocketClient, WorldId, WorldsView,
};

async fn run_socket_listener(db: UltrosDb) {
    let mut socket = WebsocketClient::connect().await;
    socket
        .subscribe(SubscribeMode::Subscribe, EventChannel::ListingsAdd, None)
        .await;
    socket
        .subscribe(SubscribeMode::Subscribe, EventChannel::ListingsRemove, None)
        .await;
    socket
        .subscribe(SubscribeMode::Subscribe, EventChannel::SalesAdd, None)
        .await;
    let receiver = socket.get_receiver();
    loop {
        if let Some(msg) = receiver.recv().await {
            // create a new task for each message
            let db = db.clone();
            tokio::spawn(async move {
                let db = &db;
                match msg {
                    SocketRx::Event(Ok(WSMessage::ListingsAdd {
                        item,
                        world,
                        listings,
                    })) => {
                        match db.update_listings(listings.clone(), item, world).await {
                            Ok((listings, num_removed)) => {
                                for listing in listings {
                                    info!("Listing added: {item:?} {world:?} {listing:?}");
                                }
                                info!("removed {num_removed}");
                            },
                            Err(e) => error!("Listing add failed {e} {listings:?}")
                        }
                    }
                    SocketRx::Event(Ok(WSMessage::ListingsRemove {
                        item,
                        world,
                        listings,
                    })) => {
                        match db.remove_listings(listings.clone(), item, world).await {
                            Ok(listings) => info!("Removed listings {listings} {item:?} {world:?}"),
                            Err(e) => error!("Error removing listings {e:?}. Listings set {listings:?} {item:?} {world:?}")
                        }
                    }
                    SocketRx::Event(Ok(WSMessage::SalesAdd { item, world, sales })) => {
                        match db.store_sale(sales.clone(), item, world).await {
                            Ok(sale) => info!("Stored sale data. Last id: {sale} {item:?} {world:?}"),
                            Err(e) => error!("Error inserting sale {e}. {sales:?} {item:?} {world:?}")
                        }
                    }
                    SocketRx::Event(Ok(WSMessage::SalesRemove { item, world, sales })) => {
                        info!("sales removed {sales:?}");
                    }
                    SocketRx::Event(Err(e)) => {
                        eprintln!("Error {e:?}");
                    }
                }
            });
        }
    }
}

async fn init_db(worlds_view: &WorldsView, datacenters: &DataCentersView) -> Result<UltrosDb> {
    info!("db starting");
    let db = UltrosDb::connect().await?;
    db.insert_default_retainer_cities().await.unwrap();
    info!("DB connected & ffxiv world data primed");
    {
        db.update_datacenters(datacenters, worlds_view).await?;
    }
    Ok(db)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Create the db before we proceed
    tracing_subscriber::fmt::init();
    let universalis_client = UniversalisClient::new();
    let (datacenters, worlds) = futures::future::join(
        universalis_client.get_data_centers(),
        universalis_client.get_worlds(),
    )
    .await;
    let worlds = worlds?;
    let datacenters = datacenters?;
    let db = init_db(&worlds, &datacenters).await.unwrap();
    // begin listening to universalis events
    tokio::spawn(run_socket_listener(db.clone()));
    tokio::spawn(start_discord(db.clone()));
    let web_state = WebState {
        db,
    };
    web::start_web(web_state).await;
    Ok(())
}
