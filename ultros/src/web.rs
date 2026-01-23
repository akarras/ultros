mod alerts_websocket;
pub(crate) mod api;
pub(crate) mod character_verifier_service;
pub(crate) mod country_code_decoder;
pub(crate) mod error;
pub(crate) mod handlers;
pub(crate) mod item_card;
pub(crate) mod oauth;
pub(crate) mod sitemap;
pub(crate) mod state;

use axum::middleware;
use axum::routing::{delete, get, post};
use axum::Router;
use leptos::prelude::provide_context;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::compression::predicate::{NotForContentType, SizeAbove};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use ultros_app::{shell, LocalWorldData};

use crate::leptos::create_leptos_app;
use crate::web::api::real_time_data::real_time_data;
use crate::web::api::{cheapest_per_world, get_best_deals, get_trends, recent_sales};
use crate::web::sitemap::{generic_pages_sitemap, item_sitemap, sitemap_index, world_sitemap};
use crate::web::{
    alerts_websocket::connect_websocket,
    item_card::item_card,
    oauth::{begin_login, logout, redirect},
};
use crate::web_metrics::{start_metrics_server, track_metrics};
pub(crate) use self::state::WebState;
use tower_http::compression::Predicate;

pub(crate) async fn start_web(state: WebState) {
    // build our application with a route
    let worlds = state.world_helper.clone();
    let token = state.token.clone();
    let app = Router::new()
        .route("/alerts/websocket", get(connect_websocket))
        .route("/api/v1/search", get(handlers::misc::search))
        .route("/api/v1/realtime/events", get(real_time_data))
        .route("/api/v1/cheapest/{world}", get(cheapest_per_world))
        .route("/api/v1/trends/{world}", get(get_trends))
        .route("/api/v1/best_deals/{world}", get(get_best_deals))
        .route("/api/v1/recentSales/{world}", get(recent_sales))
        .route(
            "/api/v1/listings/{world}/{itemid}",
            get(handlers::listing::world_item_listings),
        )
        .route(
            "/api/v1/bulkListings/{world}/{itemids}",
            get(handlers::listing::bulk_item_listings),
        )
        .route("/api/v1/list", get(handlers::list::get_lists))
        .route("/api/v1/list/create", post(handlers::list::create_list))
        .route("/api/v1/list/edit", post(handlers::list::edit_list))
        .route("/api/v1/list/item/edit", post(handlers::list::edit_list_item))
        .route("/api/v1/list/{id}", get(handlers::list::get_list))
        .route("/api/v1/list/{id}/listings", get(handlers::list::get_list_with_listings))
        .route("/api/v1/list/{id}/add/item", post(handlers::list::post_item_to_list))
        .route("/api/v1/list/{id}/add/items", post(handlers::list::post_items_to_list))
        .route("/api/v1/list/{id}/delete", delete(handlers::list::delete_list))
        .route("/api/v1/list/item/{id}/delete", delete(handlers::list::delete_list_item))
        .route("/api/v1/list/item/delete", post(handlers::list::delete_multiple_list_items))
        .route("/api/v1/world_data", get(handlers::misc::world_data))
        .route("/api/v1/current_user", get(handlers::user::current_user))
        .route("/api/v1/user/retainer", get(handlers::retainer::user_retainers))
        .route("/api/v1/retainer/reorder", post(handlers::retainer::reorder_retainer))
        .route(
            "/api/v1/user/retainer/listings",
            get(handlers::retainer::user_retainer_listings),
        )
        .route("/api/v1/retainer/search/{query}", get(handlers::retainer::retainer_search))
        .route("/api/v1/retainer/claim/{id}", get(handlers::retainer::claim_retainer))
        .route("/api/v1/retainer/unclaim/{id}", get(handlers::retainer::unclaim_retainer))
        .route(
            "/item/refresh/{worldid}/{itemid}",
            get(handlers::listing::refresh_world_item_listings),
        )
        .route("/api/v1/retainer/listings/{id}", get(handlers::retainer::retainer_listings))
        .route("/api/v1/characters/search/{name}", get(handlers::character::character_search))
        .route("/api/v1/characters/claim/{id}", get(handlers::character::claim_character))
        .route("/api/v1/characters/unclaim/{id}", get(handlers::character::unclaim_character))
        .route("/api/v1/characters/verify/{id}", get(handlers::character::verify_character))
        .route("/api/v1/characters", get(handlers::character::user_characters))
        .route(
            "/api/v1/characters/verifications",
            get(handlers::character::pending_verifications),
        )
        .route("/api/v1/detectregion", get(handlers::misc::detect_region))
        .route("/retainers/add/{id}", get(handlers::retainer::add_retainer))
        .route("/retainers/remove/{id}", get(handlers::retainer::remove_owned_retainer))
        .route("/static/{*path}", get(handlers::misc::static_path))
        .route("/static/itemicon/fallback", get(handlers::misc::fallback_item_icon))
        .route("/static/itemicon/{path}", get(handlers::misc::get_item_icon))
        .route(
            &["/static/data/", xiv_gen::data_version(), ".bincode"].concat(),
            get(handlers::misc::get_bincode),
        )
        .route("/redirect", get(redirect))
        .route("/login", get(begin_login))
        .route("/logout", get(logout))
        .route("/api/v1/current_user", delete(handlers::user::delete_user))
        .route("/invitebot", get(handlers::user::invite))
        .route("/favicon.ico", get(handlers::misc::favicon))
        .route("/robots.txt", get(handlers::misc::robots))
        .route("/itemcard/{world}/{id}", get(item_card))
        .route("/sitemap/world/{s}", get(world_sitemap))
        .route("/sitemap/items.xml", get(item_sitemap))
        .route("/sitemap.xml", get(sitemap_index))
        .route("/sitemap/pages.xml", get(generic_pages_sitemap))
        .route("/listings/{world}/{item}", get(handlers::listing::listings_redirect))
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
