pub(crate) mod alerts;
pub(crate) mod analyzer_service;
mod discord;
pub(crate) mod event;
pub(crate) mod utils;
mod web;
pub(crate) mod world_cache;

use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use crate::web::WebState;
use analyzer_service::AnalyzerService;
use anyhow::Result;
use axum_extra::extract::cookie::Key;
use discord::start_discord;
use event::{create_event_busses, EventProducer, EventType};
use tracing::{error, info};
use ultros_db::entity::active_listing;
use ultros_db::UltrosDb;
use universalis::websocket::event_types::{EventChannel, SubscribeMode, WSMessage};
use universalis::websocket::SocketRx;
use universalis::{DataCentersView, UniversalisClient, WebsocketClient, WorldsView};
use web::oauth::{AuthUserCache, DiscordAuthConfig, OAuthScope};
use world_cache::WorldCache;

async fn run_socket_listener(db: UltrosDb, listings_tx: EventProducer<Vec<active_listing::Model>>) {
    let mut socket = WebsocketClient::connect().await;
    socket
        .update_subscription(SubscribeMode::Subscribe, EventChannel::ListingsAdd, None)
        .await;
    socket
        .update_subscription(SubscribeMode::Subscribe, EventChannel::ListingsRemove, None)
        .await;
    socket
        .update_subscription(SubscribeMode::Subscribe, EventChannel::SalesAdd, None)
        .await;
    let receiver = socket.get_receiver();
    loop {
        if let Some(msg) = receiver.recv().await {
            // create a new task for each message
            let db = db.clone();
            // hopefully this is cheap to clone
            let listings_tx = listings_tx.clone();
            tokio::spawn(async move {
                let db = &db;
                match msg {
                    SocketRx::Event(Ok(WSMessage::ListingsAdd {
                        item,
                        world,
                        listings,
                    })) => {
                        match db.update_listings(listings.clone(), item, world).await {
                            Ok((listings, removed)) => {
                                let listings = Arc::new(listings);
                                let removed = Arc::new(removed);
                                match listings_tx.send(EventType::Remove(removed)) {
                                    Ok(o) => info!("sent removed listings {o} updates"),
                                    Err(e) => error!("Error removing listings {e}")
                                }
                                match listings_tx.send(EventType::Add(listings)) {
                                    Ok(o) => info!("updated listings, sent {o:?} updates"),
                                    Err(e) => error!("Error adding listings {e}")
                                };
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
                        info!("sales removed {item:?} {world:?} {sales:?}");
                    }
                    SocketRx::Event(Err(e)) => {
                        error!("Error {e:?}");
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
    let world_cache = Arc::new(WorldCache::new(&db).await);
    let (senders, receivers) = create_event_busses();
    // begin listening to universalis events
    tokio::spawn(run_socket_listener(db.clone(), senders.listings.clone()));
    tokio::spawn(start_discord(db.clone(), senders, receivers.clone()));
    // create the oauth config
    let hostname = env::var("HOSTNAME").expect(
        "Missing env variable HOSTNAME, which should be the domain of the server running this app.",
    );
    let client_id =
        env::var("DISCORD_CLIENT_ID").expect("environment variable DISCORD_CLIENT_ID not found");
    let client_secret = env::var("DISCORD_CLIENT_SECRET")
        .expect("environment variable DISCORD_CLIENT_SECRET for OAuth missing");
    let key = env::var("KEY").expect("environment variable KEY not found");
    let analyzer_service =
        AnalyzerService::start_analyzer(db.clone(), receivers.clone(), world_cache.clone()).await;
    let web_state = WebState {
        analyzer_service,
        db,
        key: Key::from(key.as_bytes()),
        oauth_config: DiscordAuthConfig::new(
            client_id,
            client_secret,
            PathBuf::from(hostname)
                .join("redirect")
                .into_os_string()
                .to_str()
                .unwrap()
                .to_string(),
            HashSet::from_iter([OAuthScope::Identify]),
        ),
        user_cache: AuthUserCache::new(),
        event_receivers: receivers,
        world_cache,
    };
    web::start_web(web_state).await;
    Ok(())
}
