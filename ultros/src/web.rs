mod alerts_websocket;
pub(crate) mod api;
pub(crate) mod character_verifier_service;
pub(crate) mod country_code_decoder;
pub(crate) mod error;
pub(crate) mod item_card;
pub(crate) mod oauth;
pub(crate) mod sitemap;

pub(crate) mod characters;
pub(crate) mod listings;
pub(crate) mod lists;
pub(crate) mod misc;
pub(crate) mod retainers;
pub(crate) mod static_files;
pub(crate) mod users;
pub(crate) mod world_data;

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::FromRef;
use axum::routing::{delete, get, post};
use axum::{Router, middleware};
use axum_extra::extract::cookie::Key;
use leptos::config::LeptosOptions;
use leptos::prelude::provide_context;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{NotForContentType, SizeAbove};
use tower_http::trace::TraceLayer;
use ultros_api_types::world_helper::WorldHelper;
use ultros_app::{LocalWorldData, shell};
use ultros_db::{UltrosDb, world_cache::WorldCache};

use self::character_verifier_service::CharacterVerifierService;
use self::oauth::{AuthUserCache, DiscordAuthConfig, OAuthScope, begin_login, logout};
use crate::analyzer_service::AnalyzerService;
use crate::event::{EventReceivers, EventSenders};
use crate::leptos::create_leptos_app;
use crate::search_service::SearchService;
use crate::web::alerts_websocket::connect_websocket;
use crate::web::api::real_time_data::real_time_data;
use crate::web::api::{cheapest_per_world, get_best_deals, get_trends, recent_sales};
use crate::web::item_card::item_card;
use crate::web::sitemap::{generic_pages_sitemap, item_sitemap, sitemap_index, world_sitemap};
use crate::web_metrics::{start_metrics_server, track_metrics};

#[derive(Clone)]
pub(crate) struct WebState {
    pub(crate) db: UltrosDb,
    pub(crate) key: Key,
    pub(crate) oauth_config: DiscordAuthConfig,
    pub(crate) user_cache: AuthUserCache,
    pub(crate) event_receivers: EventReceivers,
    pub(crate) event_senders: EventSenders,
    pub(crate) world_cache: Arc<WorldCache>,
    /// Common variant of world_cache. Maybe get rid of world_cache?
    pub(crate) world_helper: Arc<WorldHelper>,
    pub(crate) analyzer_service: AnalyzerService,
    pub(crate) character_verification: CharacterVerifierService,
    pub(crate) leptos_options: LeptosOptions,
    pub(crate) search_service: SearchService,
    pub(crate) token: CancellationToken,
}

impl FromRef<WebState> for UltrosDb {
    fn from_ref(input: &WebState) -> Self {
        input.db.clone()
    }
}

impl FromRef<WebState> for Key {
    fn from_ref(input: &WebState) -> Self {
        input.key.clone()
    }
}

impl FromRef<WebState> for DiscordAuthConfig {
    fn from_ref(input: &WebState) -> Self {
        input.oauth_config.clone()
    }
}

impl FromRef<WebState> for AuthUserCache {
    fn from_ref(input: &WebState) -> Self {
        input.user_cache.clone()
    }
}

impl FromRef<WebState> for EventReceivers {
    fn from_ref(input: &WebState) -> Self {
        input.event_receivers.clone()
    }
}

impl FromRef<WebState> for Arc<WorldCache> {
    fn from_ref(input: &WebState) -> Self {
        input.world_cache.clone()
    }
}

impl FromRef<WebState> for Arc<WorldHelper> {
    fn from_ref(input: &WebState) -> Self {
        input.world_helper.clone()
    }
}

impl FromRef<WebState> for AnalyzerService {
    fn from_ref(input: &WebState) -> Self {
        input.analyzer_service.clone()
    }
}

impl FromRef<WebState> for EventSenders {
    fn from_ref(input: &WebState) -> Self {
        input.event_senders.clone()
    }
}

impl FromRef<WebState> for CharacterVerifierService {
    fn from_ref(input: &WebState) -> Self {
        input.character_verification.clone()
    }
}

impl FromRef<WebState> for LeptosOptions {
    fn from_ref(input: &WebState) -> Self {
        input.leptos_options.clone()
    }
}

impl FromRef<WebState> for SearchService {
    fn from_ref(input: &WebState) -> Self {
        input.search_service.clone()
    }
}

pub(crate) async fn start_web(state: WebState) {
    // build our application with a route
    let worlds = state.world_helper.clone();
    let token = state.token.clone();
    let app = Router::new()
        .route("/alerts/websocket", get(connect_websocket))
        .route("/api/v1/search", get(misc::search))
        .route("/api/v1/realtime/events", get(real_time_data))
        .route("/api/v1/cheapest/{world}", get(cheapest_per_world))
        .route("/api/v1/trends/{world}", get(get_trends))
        .route("/api/v1/best_deals/{world}", get(get_best_deals))
        .route("/api/v1/recentSales/{world}", get(recent_sales))
        .route(
            "/api/v1/listings/{world}/{itemid}",
            get(listings::world_item_listings),
        )
        .route(
            "/api/v1/bulkListings/{world}/{itemids}",
            get(listings::bulk_item_listings),
        )
        .route("/api/v1/list", get(lists::get_lists))
        .route("/api/v1/list/create", post(lists::create_list))
        .route("/api/v1/list/edit", post(lists::edit_list))
        .route("/api/v1/list/item/edit", post(lists::edit_list_item))
        .route("/api/v1/list/{id}", get(lists::get_list))
        .route(
            "/api/v1/list/{id}/listings",
            get(lists::get_list_with_listings),
        )
        .route("/api/v1/list/{id}/add/item", post(lists::post_item_to_list))
        .route(
            "/api/v1/list/{id}/add/items",
            post(lists::post_items_to_list),
        )
        .route("/api/v1/list/{id}/delete", delete(lists::delete_list))
        .route(
            "/api/v1/list/item/{id}/delete",
            delete(lists::delete_list_item),
        )
        .route(
            "/api/v1/list/item/delete",
            post(lists::delete_multiple_list_items),
        )
        .route("/api/v1/world_data", get(world_data::world_data))
        .route("/api/v1/current_user", get(users::current_user))
        .route("/api/v1/user/retainer", get(retainers::user_retainers))
        .route(
            "/api/v1/retainer/reorder",
            post(retainers::reorder_retainer),
        )
        .route(
            "/api/v1/user/retainer/listings",
            get(retainers::user_retainer_listings),
        )
        .route(
            "/api/v1/retainer/search/{query}",
            get(retainers::retainer_search),
        )
        .route(
            "/api/v1/retainer/claim/{id}",
            get(retainers::claim_retainer),
        )
        .route(
            "/api/v1/retainer/unclaim/{id}",
            get(retainers::unclaim_retainer),
        )
        .route(
            "/item/refresh/{worldid}/{itemid}",
            get(listings::refresh_world_item_listings),
        )
        .route(
            "/api/v1/retainer/listings/{id}",
            get(retainers::retainer_listings),
        )
        .route(
            "/api/v1/characters/search/{name}",
            get(characters::character_search),
        )
        .route(
            "/api/v1/characters/claim/{id}",
            get(characters::claim_character),
        )
        .route(
            "/api/v1/characters/unclaim/{id}",
            get(characters::unclaim_character),
        )
        .route(
            "/api/v1/characters/verify/{id}",
            get(characters::verify_character),
        )
        .route("/api/v1/characters", get(characters::user_characters))
        .route(
            "/api/v1/characters/verifications",
            get(characters::pending_verifications),
        )
        .route("/api/v1/detectregion", get(world_data::detect_region))
        .route("/retainers/add/{id}", get(retainers::add_retainer))
        .route(
            "/retainers/remove/{id}",
            get(retainers::remove_owned_retainer),
        )
        .route("/static/{*path}", get(static_files::static_path))
        .route(
            "/static/itemicon/fallback",
            get(static_files::fallback_item_icon),
        )
        .route("/static/itemicon/{path}", get(static_files::get_item_icon))
        .route(
            &["/static/data/", xiv_gen::data_version(), ".bincode"].concat(),
            get(static_files::get_bincode),
        )
        .route("/redirect", get(self::oauth::redirect))
        .route("/login", get(begin_login))
        .route("/logout", get(logout))
        .route("/api/v1/current_user", delete(users::delete_user))
        .route("/invitebot", get(misc::invite))
        .route("/favicon.ico", get(static_files::favicon))
        .route("/robots.txt", get(static_files::robots))
        .route("/itemcard/{world}/{id}", get(item_card))
        .route("/sitemap/world/{s}", get(world_sitemap))
        .route("/sitemap/items.xml", get(item_sitemap))
        .route("/sitemap.xml", get(sitemap_index))
        .route("/sitemap/pages.xml", get(generic_pages_sitemap))
        .route("/listings/{world}/{item}", get(listings::listings_redirect))
        .merge(create_leptos_app(state.world_helper.clone()).await.unwrap())
        .fallback(leptos_axum::file_and_error_handler_with_context::<
            WebState,
            _,
        >(
            move || {
                provide_context(LocalWorldData(Ok(worlds.clone())));
            },
            shell,
        ))
        .with_state(state)
        .route_layer(middleware::from_fn(track_metrics))
        .layer(TraceLayer::new_for_http())
        .layer(
            CompressionLayer::new().compress_when(
                SizeAbove::new(256)
                    // don't compress images
                    .and(NotForContentType::IMAGES),
            ),
        );

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    let port = std::env::var("PORT")
        .map(|p| p.parse::<u16>().ok())
        .ok()
        .flatten()
        .unwrap_or(8080);
    let (_main_app, _metrics_app) = futures::future::join(
        async move {
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            tracing::info!("listening on {}", addr);
            let listener = TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    token.cancelled().await;
                })
                .await
                .unwrap();
        },
        start_metrics_server(),
    )
    .await;
}
