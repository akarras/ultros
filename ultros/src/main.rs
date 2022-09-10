mod web;

use crate::web::WebState;
use anyhow::Result;
use futures::future::join_all;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};
use ultros_db::UltrosDb;
use universalis::websocket::event_types::{EventChannel, SaleView, SubscribeMode, WSMessage};
use universalis::websocket::SocketRx;
use universalis::{
    DataCenterView, DataCentersView, ItemId, ListingView, UniversalisClient, WebsocketClient,
    WorldId, WorldsView,
};

async fn process_sales(db: &UltrosDb, sales: Vec<SaleView>, item_id: ItemId, world_id: WorldId) {}

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
    info!("DB connected & ffxiv world data primed");
    {
        let db = &db;
        let regions: HashSet<String> = datacenters.0.iter().map(|m| m.region.0.clone()).collect();
        let regions = join_all(
            regions
                .iter()
                .map(|m| async move { db.store_region(m).await }),
        )
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
        info!("inserted regions {regions:?}");
        
        let dcs = join_all(
            datacenters
                .0
                .iter()
                .map(|dc| async move { db.store_datacenter(&dc.name.0, &dc.region.0).await }),
        )
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
        info!("inserted datacenters {datacenters:?}");
        let worlds = join_all(worlds_view.0.iter().map(|world| {
            db.store_world(
                world.id,
                &world.name.0,
                &datacenters
                    .0
                    .iter()
                    .find(|dc| dc.worlds.contains(&world.id))
                    .unwrap()
                    .name
                    .0,
            )
        }))
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
        info!("inserted worlds {worlds:?}");
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
    let worlds = Arc::new(worlds);
    let datacenters = Arc::new(datacenters);
    // begin listening to universalis events
    tokio::spawn(run_socket_listener(db.clone()));
    let web_state = WebState {
        db,
        worlds,
        datacenters,
    };
    web::start_web(web_state).await;
    Ok(())
}
