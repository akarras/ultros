pub(crate) mod alerts;
pub(crate) mod analyzer_service;
mod discord;
pub(crate) mod event;
mod item_update_service;
pub mod leptos;
pub(crate) mod utils;
mod web;
mod web_metrics;

use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use crate::item_update_service::UpdateService;
use crate::web::WebState;
use analyzer_service::AnalyzerService;
use anyhow::Result;
use axum_extra::extract::cookie::Key;
use discord::start_discord;
use event::{create_event_busses, EventProducer, EventType};
use tracing::{error, info};
use ultros_db::entity::{active_listing, sale_history};
use ultros_db::world_cache::WorldCache;
use ultros_db::UltrosDb;
use universalis::websocket::event_types::{EventChannel, SubscribeMode, WSMessage};
use universalis::websocket::SocketRx;
use universalis::{DataCentersView, UniversalisClient, WebsocketClient, WorldId, WorldsView};
use web::character_verifier_service::CharacterVerifierService;
use web::oauth::{AuthUserCache, DiscordAuthConfig, OAuthScope};

async fn run_socket_listener(
    db: UltrosDb,
    listings_tx: EventProducer<Vec<active_listing::Model>>,
    sales_tx: EventProducer<Vec<sale_history::Model>>,
) {
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
            let sales_tx = sales_tx.clone();
            if let SocketRx::Event(Ok(e)) = &msg {
                let world_id = WorldId::from(e);
                metrics::counter!("ultros_websocket_rx", 1, "WorldId" => world_id.0.to_string());
            }
            tokio::spawn(async move {
                let db = &db;
                match msg {
                    SocketRx::Event(Ok(WSMessage::ListingsAdd {
                        item,
                        world,
                        listings,
                    })) => match db.update_listings(listings.clone(), item, world).await {
                        Ok((listings, removed)) => {
                            let listings = Arc::new(listings);
                            let removed = Arc::new(removed);
                            match listings_tx.send(EventType::Remove(removed)) {
                                Ok(o) => info!("sent removed listings {o} updates"),
                                Err(e) => error!("Error removing listings {e}"),
                            }
                            match listings_tx.send(EventType::Add(listings)) {
                                Ok(o) => info!("updated listings, sent {o:?} updates"),
                                Err(e) => error!("Error adding listings {e}"),
                            };
                        }
                        Err(e) => error!("Listing add failed {e} {listings:?}"),
                    },
                    SocketRx::Event(Ok(WSMessage::ListingsRemove {
                        item,
                        world,
                        listings,
                    })) => {
                        match db.remove_listings(listings.clone(), item, world).await {
                            Ok(listings) => {
                                info!("Removed listings {listings:?} {item:?} {world:?}");
                                if let Err(e) = listings_tx.send(EventType::Remove(Arc::new(listings))) {
                                    error!("Error sending remove listings {e:?}");
                                }
                            },
                            Err(e) => error!("Error removing listings {e:?}. Listings set {listings:?} {item:?} {world:?}")
                        }
                    }
                    SocketRx::Event(Ok(WSMessage::SalesAdd { item, world, sales })) => {
                        match db.store_sale(sales.clone(), item, world).await {
                            Ok(added_sales) => {
                                info!(
                                    "Stored sale data. Last id: {added_sales:?} {item:?} {world:?}"
                                );
                                match sales_tx.send(EventType::Add(Arc::new(added_sales))) {
                                    Ok(o) => info!("Sent sale {o} updates"),
                                    Err(e) => error!("Error sending sale update {e:?}"),
                                }
                            }
                            Err(e) => {
                                error!("Error inserting sale {e}. {sales:?} {item:?} {world:?}")
                            }
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

async fn init_db(
    db: &UltrosDb,
    worlds_view: Result<WorldsView, universalis::Error>,
    datacenters: Result<DataCentersView, universalis::Error>,
) -> Result<()> {
    info!("db starting");

    db.insert_default_retainer_cities().await.unwrap();
    info!("DB connected & ffxiv world data primed");
    {
        if let (Ok(worlds), Ok(datacenters)) = (worlds_view, datacenters) {
            db.update_datacenters(&datacenters, &worlds).await?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Create the db before we proceed
    tracing_subscriber::fmt::init();
    info!("Ultros starting!");
    info!("Connecting DB");
    let db = UltrosDb::connect().await?;
    info!("Fetching datacenters/worlds from universalis");
    let universalis_client = UniversalisClient::new();
    let init = db.clone();
    let (senders, receivers) = create_event_busses();
    let listings_sender = senders.listings.clone();
    let history_sender = senders.history.clone();
    tokio::spawn(async move {
        let (datacenters, worlds) = futures::future::join(
            universalis_client.get_data_centers(),
            universalis_client.get_worlds(),
        )
        .await;
        info!("Initializing database with worlds/datacenters");
        init_db(&init, worlds, datacenters)
            .await
            .expect("Unable to populate worlds datacenters- is universalis down?");
        info!("starting websocket");
        run_socket_listener(init, listings_sender, history_sender).await;
    });
    // on first run, the world cache may be empty
    let world_cache = Arc::new(WorldCache::new(&db).await);

    let analyzer_service =
        AnalyzerService::start_analyzer(db.clone(), receivers.clone(), world_cache.clone()).await;
    // begin listening to universalis events
    tokio::spawn(start_discord(
        db.clone(),
        senders.clone(),
        receivers.clone(),
        analyzer_service.clone(),
        world_cache.clone(),
    ));
    // create the oauth config
    let hostname = env::var("HOSTNAME").expect(
        "Missing env variable HOSTNAME, which should be the domain of the server running this app.",
    );
    let client_id =
        env::var("DISCORD_CLIENT_ID").expect("environment variable DISCORD_CLIENT_ID not found");
    let client_secret = env::var("DISCORD_CLIENT_SECRET")
        .expect("environment variable DISCORD_CLIENT_SECRET for OAuth missing");
    let key = env::var("KEY").expect("environment variable KEY not found");
    let character_verification = CharacterVerifierService {
        client: reqwest::Client::new(),
        db: db.clone(),
        world_cache: world_cache.clone(),
    };
    UpdateService::start_service(db.clone(), world_cache.clone());

    let web_state = WebState {
        analyzer_service,
        db,
        key: Key::from(key.as_bytes()),
        character_verification,
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
        event_senders: senders,
        world_cache,
    };
    web::start_web(web_state).await;
    Ok(())
}
