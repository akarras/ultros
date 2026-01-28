#![feature(trivial_bounds)]
#![recursion_limit = "256"]
pub(crate) mod alerts;
pub(crate) mod analyzer_service;
mod db_init;
mod discord;
pub(crate) mod event;
mod item_update_service;
pub mod leptos;
#[cfg(feature = "profiling")]
pub mod profiling;
pub(crate) mod search_service;
mod socket_listener;
pub(crate) mod utils;
mod web;
mod web_metrics;

use crate::db_init::init_db;
use crate::item_update_service::UpdateService;
#[cfg(feature = "profiling")]
use crate::profiling::start_profiling_server;
use crate::search_service::SearchService;
use crate::socket_listener::run_socket_listener;
use crate::web::WebState;
use ::leptos::config::get_configuration;
use analyzer_service::AnalyzerService;
use anyhow::Result;
use axum_extra::extract::cookie::Key;
use discord::start_discord;
use dotenvy::dotenv;
use event::create_event_busses;
use std::collections::HashSet;
use std::sync::Arc;
#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
use tikv_jemallocator::Jemalloc;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_subscriber::EnvFilter;
use ultros_api_types::world::WorldData;
use ultros_api_types::world_helper::WorldHelper;
use ultros_db::UltrosDb;
use ultros_db::world_cache::WorldCache;
use universalis::UniversalisClient;
use web::character_verifier_service::CharacterVerifierService;
use web::oauth::{AuthUserCache, DiscordAuthConfig, OAuthScope};
#[cfg(all(not(target_env = "msvc"), feature = "jemalloc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;
#[cfg(feature = "profiling")]
#[allow(non_upper_case_globals)]
#[export_name = "malloc_conf"]
pub static malloc_conf: &[u8] = b"prof:true,prof_active:true,lg_prof_sample:19\0";

#[derive(Debug, serde::Deserialize, Clone)]
struct Config {
    hostname: String,
    discord_client_id: String,
    discord_client_secret: String,
    key: String,
    discord_token: String,
}

// Bolt: Switched to multi-threaded runtime for better performance on multi-core systems
#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from `.env` file, if present
    dotenv().ok();

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
    let token = CancellationToken::new();
    let socket_token = token.clone();
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
        run_socket_listener(init, listings_sender, history_sender, socket_token).await;
    });
    // on first run, the world cache may be empty
    let world_cache = Arc::new(WorldCache::new(&db).await);
    let world_helper = Arc::new(WorldHelper::new(WorldData::from(world_cache.as_ref())));
    let analyzer_service = AnalyzerService::start_analyzer(
        db.clone(),
        receivers.clone(),
        world_cache.clone(),
        token.clone(),
    )
    .await;
    let update_service = Arc::new(UpdateService {
        db: db.clone(),
        world_cache: world_cache.clone(),
        universalis: UniversalisClient::new("ultros"),
        listings: senders.listings.clone(),
        sales: senders.history.clone(),
    });
    UpdateService::start_service(update_service.clone(), token.clone());
    // begin listening to universalis events
    // load configuration from environment
    let config = envy::from_env::<Config>()?;
    let Config {
        hostname,
        discord_client_id,
        discord_client_secret,
        key,
        discord_token,
    } = config;

    tokio::spawn(start_discord(
        db.clone(),
        senders.clone(),
        receivers.clone(),
        analyzer_service.clone(),
        world_cache.clone(),
        world_helper.clone(),
        update_service,
        discord_token,
        token.clone(),
    ));

    let character_verification = CharacterVerifierService {
        client: reqwest::Client::new(),
        db: db.clone(),
        world_cache: world_cache.clone(),
    };
    let search_service = SearchService::new()?;
    let conf = get_configuration(Some("Cargo.toml")).unwrap();
    let mut leptos_options = conf.leptos_options;
    let git_hash = git_const::git_short_hash!();
    leptos_options.site_pkg_dir = Arc::from(["pkg/", git_hash].concat());
    let web_state = WebState {
        analyzer_service,
        db,
        key: Key::from(key.as_bytes()),
        character_verification,
        oauth_config: DiscordAuthConfig::new(
            discord_client_id,
            discord_client_secret,
            format!("{}/redirect", hostname.trim_end_matches('/')),
            HashSet::from_iter([OAuthScope::Identify]),
        ),
        user_cache: AuthUserCache::new(),
        event_receivers: receivers,
        event_senders: senders,
        world_cache,
        world_helper,
        leptos_options,
        search_service,
        token: token.clone(),
    };
    let web_task = tokio::spawn(web::start_web(web_state));
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("ctrl-c received");
        }
        _ = web_task => {
            info!("web task finished");
        }
    }
    token.cancel();
    info!("Exiting");
    Ok(())
}
