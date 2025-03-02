#![recursion_limit = "256"]
pub(crate) mod alerts;
pub(crate) mod analyzer_service;
mod discord;
pub(crate) mod event;
mod item_update_service;
pub mod leptos;
#[cfg(feature = "profiling")]
pub mod profiling;
pub(crate) mod utils;
mod web;
mod web_metrics;

use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use crate::item_update_service::UpdateService;
#[cfg(feature = "profiling")]
use crate::profiling::start_profiling_server;
use crate::web::WebState;
use ::leptos::config::get_configuration;
use analyzer_service::AnalyzerService;
use anyhow::Result;
use axum_extra::extract::cookie::Key;
use discord::start_discord;
use event::{create_event_busses, EventProducer, EventType};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;
use ultros_api_types::websocket::{ListingEventData, SaleEventData};
use ultros_api_types::world::WorldData;
use ultros_api_types::world_helper::WorldHelper;
use ultros_db::world_cache::WorldCache;
use ultros_db::UltrosDb;
use universalis::websocket::event_types::{EventChannel, SubscribeMode, WSMessage};
use universalis::websocket::SocketRx;
use universalis::{DataCentersView, UniversalisClient, WebsocketClient, WorldId, WorldsView};
use web::character_verifier_service::CharacterVerifierService;
use web::oauth::{AuthUserCache, DiscordAuthConfig, OAuthScope};

#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

#[cfg(feature = "profiling")]
#[allow(non_upper_case_globals)]
#[export_name = "malloc_conf"]
pub static malloc_conf: &[u8] = b"prof:true,prof_active:true,lg_prof_sample:19\0";

async fn run_socket_listener(
    db: UltrosDb,
    listings_tx: EventProducer<ListingEventData>,
    sales_tx: EventProducer<SaleEventData>,
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
                metrics::counter!("ultros_websocket_rx", "WorldId" => world_id.0.to_string())
                    .increment(1);
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
                            let listings = Arc::new(ListingEventData {
                                item_id: item.0,
                                world_id: world.0,
                                listings,
                            });
                            let removed = Arc::new(ListingEventData {
                                item_id: item.0,
                                world_id: world.0,
                                listings: removed,
                            });
                            match listings_tx.send(EventType::Remove(removed)) {
                                Ok(o) => info!(slack_remaining = o, "sent removed listings"),
                                Err(e) => error!(error = ?e, "Error removing listings"),
                            }
                            match listings_tx.send(EventType::Add(listings)) {
                                Ok(o) => info!(remaining_slack = o, "updated listings"),
                                Err(e) => error!(error = ?e, "Error adding listings"),
                            };
                        }
                        Err(e) => error!(error = ?e, listings = ?listings, "Listing add failed"),
                    },
                    SocketRx::Event(Ok(WSMessage::ListingsRemove {
                        item,
                        world,
                        listings,
                    })) => match db.remove_listings(listings.clone(), item, world).await {
                        Ok(listings) => {
                            info!(?listings, ?item, ?world, "Removed listings");
                            if let Err(e) = listings_tx.send(EventType::removed(ListingEventData {
                                item_id: item.0,
                                world_id: world.0,
                                listings,
                            })) {
                                error!(error = ?e, "Error sending remove listings");
                            }
                        }
                        Err(e) => {
                            error!(error = ?e, ?listings, ?item, ?world, "Error removing listings. Listings set")
                        }
                    },
                    SocketRx::Event(Ok(WSMessage::SalesAdd { item, world, sales })) => {
                        match db.update_sales(sales.clone(), item, world).await {
                            Ok(added_sales) => {
                                info!(?added_sales, ?item, ?world, "Stored sale data");
                                match sales_tx
                                    .send(EventType::added(SaleEventData { sales: added_sales }))
                                {
                                    Ok(o) => info!(slack_remaining = o, "Sent sale"),
                                    Err(e) => error!(error = ?e, "Error sending sale update"),
                                }
                            }
                            Err(e) => {
                                error!(error = ?e, ?sales, ?item, ?world, "Error inserting sale.")
                            }
                        }
                    }
                    SocketRx::Event(Ok(WSMessage::SalesRemove { item, world, sales })) => {
                        info!(?item, ?world, ?sales, "sales removed");
                    }
                    SocketRx::Event(Err(e)) => {
                        error!(error = ?e, "Error");
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
    let filter: EnvFilter =
        EnvFilter::try_from_default_env().unwrap_or("warn,ultros=info,ultros-app=info".into());
    tracing_subscriber::fmt::fmt()
        .with_file(true)
        .with_line_number(true)
        .with_env_filter(filter)
        .pretty()
        .init();
    #[cfg(feature = "profiling")]
    tokio::spawn(async move { start_profiling_server().await });
    info!("Ultros starting!");
    info!("Connecting DB");
    let db = UltrosDb::connect().await?;
    info!("Fetching datacenters/worlds from universalis");
    let universalis_client = UniversalisClient::new("ultros");
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
    let world_helper = Arc::new(WorldHelper::new(WorldData::from(world_cache.as_ref())));

    let analyzer_service =
        AnalyzerService::start_analyzer(db.clone(), receivers.clone(), world_cache.clone()).await;
    let update_service = Arc::new(UpdateService {
        db: db.clone(),
        world_cache: world_cache.clone(),
        universalis: UniversalisClient::new("ultros"),
        listings: senders.listings.clone(),
        sales: senders.history.clone(),
    });
    UpdateService::start_service(update_service.clone());
    // begin listening to universalis events
    tokio::spawn(start_discord(
        db.clone(),
        senders.clone(),
        receivers.clone(),
        analyzer_service.clone(),
        world_cache.clone(),
        world_helper.clone(),
        update_service,
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
    let conf = get_configuration(Some("Cargo.toml")).unwrap();
    let leptos_options = conf.leptos_options;
    // let addr = leptos_options.site_addr;
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
        world_helper,
        leptos_options,
    };
    web::start_web(web_state).await;
    Ok(())
}
