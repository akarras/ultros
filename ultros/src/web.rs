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

use axum::routing::{delete, get, post};
use axum::{Router, middleware};
use leptos::prelude::provide_context;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::compression::CompressionLayer;
use tower_http::compression::Predicate;
use tower_http::compression::predicate::{NotForContentType, SizeAbove};
use tower_http::trace::TraceLayer;
use ultros_app::{LocalWorldData, shell};

use crate::leptos::create_leptos_app;
use crate::web::alerts_websocket::connect_websocket;
use crate::web::api::real_time_data::real_time_data;
use crate::web::api::{cheapest_per_world, get_best_deals, get_trends, recent_sales};
use crate::web::item_card::item_card;
use crate::web::oauth::{begin_login, logout};
use crate::web::sitemap::{generic_pages_sitemap, item_sitemap, sitemap_index, world_sitemap};
use crate::web_metrics::{start_metrics_server, track_metrics};

use handlers::{character, list, listing, misc, retainer, user};
pub(crate) use state::WebState;

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
            get(listing::world_item_listings),
        )
        .route(
            "/api/v1/bulkListings/{world}/{itemids}",
            get(listing::bulk_item_listings),
        )
        .route("/api/v1/list", get(list::get_lists))
        .route("/api/v1/list/create", post(list::create_list))
        .route("/api/v1/list/edit", post(list::edit_list))
        .route("/api/v1/list/item/edit", post(list::edit_list_item))
        .route("/api/v1/list/{id}", get(list::get_list))
        .route(
            "/api/v1/list/{id}/listings",
            get(list::get_list_with_listings),
        )
        .route("/api/v1/list/{id}/add/item", post(list::post_item_to_list))
        .route(
            "/api/v1/list/{id}/add/items",
            post(list::post_items_to_list),
        )
        .route("/api/v1/list/{id}/delete", delete(list::delete_list))
        .route(
            "/api/v1/list/item/{id}/delete",
            delete(list::delete_list_item),
        )
        .route(
            "/api/v1/list/item/delete",
            post(list::delete_multiple_list_items),
        )
        .route("/api/v1/world_data", get(misc::world_data))
        .route("/api/v1/current_user", get(user::current_user))
        .route("/api/v1/user/retainer", get(retainer::user_retainers))
        .route("/api/v1/retainer/reorder", post(retainer::reorder_retainer))
        .route(
            "/api/v1/user/retainer/listings",
            get(retainer::user_retainer_listings),
        )
        .route(
            "/api/v1/retainer/search/{query}",
            get(retainer::retainer_search),
        )
        .route("/api/v1/retainer/claim/{id}", get(retainer::claim_retainer))
        .route(
            "/api/v1/retainer/unclaim/{id}",
            get(retainer::unclaim_retainer),
        )
        .route(
            "/item/refresh/{worldid}/{itemid}",
            get(listing::refresh_world_item_listings),
        )
        .route(
            "/api/v1/retainer/listings/{id}",
            get(retainer::retainer_listings),
        )
        .route(
            "/api/v1/characters/search/{name}",
            get(character::character_search),
        )
        .route(
            "/api/v1/characters/claim/{id}",
            get(character::claim_character),
        )
        .route(
            "/api/v1/characters/unclaim/{id}",
            get(character::unclaim_character),
        )
        .route(
            "/api/v1/characters/verify/{id}",
            get(character::verify_character),
        )
        .route("/api/v1/characters", get(character::user_characters))
        .route(
            "/api/v1/characters/verifications",
            get(character::pending_verifications),
        )
        .route("/api/v1/detectregion", get(misc::detect_region))
        .route("/retainers/add/{id}", get(retainer::add_retainer))
        .route(
            "/retainers/remove/{id}",
            get(retainer::remove_owned_retainer),
        )
        .route("/static/{*path}", get(misc::static_path))
        .route("/static/itemicon/fallback", get(misc::fallback_item_icon))
        .route("/static/itemicon/{path}", get(misc::get_item_icon))
        .route(
            &["/static/data/", xiv_gen::data_version(), ".bincode"].concat(),
            get(misc::get_bincode),
        )
        .route("/redirect", get(self::oauth::redirect))
        .route("/login", get(begin_login))
        .route("/logout", get(logout))
        .route("/api/v1/current_user", delete(user::delete_user))
        .route("/invitebot", get(user::invite))
        .route("/favicon.ico", get(misc::favicon))
        .route("/robots.txt", get(misc::robots))
        .route("/itemcard/{world}/{id}", get(item_card))
        .route("/sitemap/world/{s}", get(world_sitemap))
        .route("/sitemap/items.xml", get(item_sitemap))
        .route("/sitemap.xml", get(sitemap_index))
        .route("/sitemap/pages.xml", get(generic_pages_sitemap))
        .route("/listings/{world}/{item}", get(listing::listings_redirect))
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
