use std::{error::Error, sync::Arc};

#[cfg(not(debug_assertions))]
use axum::http::HeaderValue;
/// Ultros UI server contains all the axum routes required to serve and bundle leptos wasm files
/// # Building
/// I recommend you use cargo-leptos, once you go through the steps to install cargo-leptos
/// you should be able to build and serve leptos with one install step.
///
use axum::{
    body::Body,
    extract::State,
    http::Request,
    response::{IntoResponse, Response},
    routing::get,
    Extension, Router,
};
use git_const::git_short_hash;
#[cfg(not(debug_assertions))]
use hyper::header;
use leptos::prelude::*;
use leptos_axum::{generate_route_list, LeptosRoutes};
use leptos_router::RouteListing;
#[cfg(not(debug_assertions))]
use tower_http::set_header::SetResponseHeader;
use tracing::instrument;
use ultros_api_types::world_helper::WorldHelper;
use ultros_app::*;

use crate::web::{country_code_decoder::Region, WebState};

#[instrument(skip(worlds, options, req))]
#[axum::debug_handler]
async fn custom_handler(
    State(worlds): State<Arc<WorldHelper>>,
    Extension(options): Extension<Arc<LeptosOptions>>,
    region: Option<Region>,
    req: Request<Body>,
) -> Response {
    let handler = leptos_axum::render_app_to_stream(
        move || view! { <App worlds=Ok(worlds.clone()) region=region.unwrap_or(Region::NorthAmerica).to_string()/> },
    );
    handler(req).await.into_response()
}

pub(crate) async fn create_leptos_app(
    worlds: Arc<WorldHelper>,
) -> Result<Router<WebState>, Box<dyn Error>> {
    use axum::http::StatusCode;
    use tower_http::services::ServeDir;

    let conf = get_configuration(None)?;
    let mut leptos_options = conf.leptos_options;
    let site_root = &leptos_options.site_root;
    let pkg_dir = &leptos_options.site_pkg_dir;

    // The URL path of the generated JS/WASM bundle from cargo-leptos
    let bundle_path = format!("/{site_root}/{pkg_dir}");
    // The filesystem path of the generated JS/WASM bundle from cargo-leptos
    let bundle_filepath = format!("./{site_root}/{pkg_dir}");
    let addr = leptos_options.site_addr;
    tracing::debug!("serving at {addr}");

    // simple_logger::init_with_level(log::Level::Debug).expect("couldn't initialize logging");

    // These are Tower Services that will serve files from the static and pkg repos.
    // HandleError is needed as Axum requires services to implement Infallible Errors
    // because all Errors are converted into Responses
    // let static_service = HandleError::new(ServeDir::new("./static"), handle_file_error);
    //let pkg_service = HandleError::new(ServeDir::new("./pkg"), handle_file_error);
    let git_hash = git_short_hash!();
    leptos_options.site_pkg_dir = Arc::from(["pkg/", git_hash].concat());
    // let cargo_leptos_service = HandleError::new(ServeDir::new(&bundle_filepath), handle_file_error);
    let cargo_leptos_service = ServeDir::new(&bundle_filepath);
    #[cfg(not(debug_assertions))]
    let cargo_leptos_service = SetResponseHeader::appending(
        cargo_leptos_service,
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400, immutable"),
    );
    tracing::info!("Serving pkg dir: {bundle_filepath}");
    /// Convert the Errors from ServeDir to a type that implements IntoResponse
    async fn handle_file_error(err: std::io::Error) -> (StatusCode, String) {
        (StatusCode::NOT_FOUND, format!("File Not Found: {}", err))
    }
    let worlds = Ok(worlds);
    let routes = generate_route_list(move || {
        let worlds = worlds.clone();
        view! { <App worlds region="North-America".to_string()/> }
    });

    // simple_logger::init_with_level(log::Level::Debug).expect("couldn't initialize logging");

    // build our application with a route
    Ok(Router::new()
        // `GET /` goes to `root`
        .nest_service(
            &["/", &leptos_options.site_pkg_dir].concat(),
            cargo_leptos_service.clone(),
        ) // Only need if using wasm-pack. Can be deleted if using cargo-leptos
        .nest_service(&bundle_path, cargo_leptos_service) // Only needed if using cargo-leptos. Can be deleted if using wasm-pack and cargo-run
        //.nest_service("/static", static_service)
        .leptos_routes_with_handler(routes, custom_handler))
    // .with_state(state)
    // .layer(Extension(Arc::new(leptos_options))))
}
