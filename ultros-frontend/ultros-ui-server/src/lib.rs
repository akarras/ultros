/// Ultros UI server contains all the axum routes required to serve and bundle leptos wasm files
/// # Building
/// I recommend you use cargo-leptos, once you go through the steps to install cargo-leptos
/// you should be able to build and serve leptos with one install step.
///
use axum::Router;
use leptos::*;
use leptos_axum::{generate_route_list, LeptosRoutes};
use ultros_app::*;

pub async fn create_leptos_app(_db: ultros_db::UltrosDb) -> Router {
    use axum::{error_handling::HandleError, http::StatusCode};
    use tower_http::services::ServeDir;

    let conf = get_configuration(None).await.unwrap();
    let leptos_options = conf.leptos_options;
    let site_root = &leptos_options.site_root;
    let pkg_dir = &leptos_options.site_pkg_dir;

    // The URL path of the generated JS/WASM bundle from cargo-leptos
    let bundle_path = format!("/{site_root}/{pkg_dir}");
    // The filesystem path of the generated JS/WASM bundle from cargo-leptos
    let bundle_filepath = format!("./{site_root}/{pkg_dir}");
    let addr = leptos_options.site_addr.clone();
    log::debug!("serving at {addr}");

    // simple_logger::init_with_level(log::Level::Debug).expect("couldn't initialize logging");

    // These are Tower Services that will serve files from the static and pkg repos.
    // HandleError is needed as Axum requires services to implement Infallible Errors
    // because all Errors are converted into Responses
    // let static_service = HandleError::new(ServeDir::new("./static"), handle_file_error);
    //let pkg_service = HandleError::new(ServeDir::new("./pkg"), handle_file_error);
    let cargo_leptos_service = HandleError::new(ServeDir::new(&bundle_filepath), handle_file_error);
    log::info!("Serving pkg dir: {bundle_filepath}");
    /// Convert the Errors from ServeDir to a type that implements IntoResponse
    async fn handle_file_error(err: std::io::Error) -> (StatusCode, String) {
        (StatusCode::NOT_FOUND, format!("File Not Found: {}", err))
    }

    let routes = generate_route_list(|cx| view! { cx, <App/> }).await;

    // simple_logger::init_with_level(log::Level::Debug).expect("couldn't initialize logging");

    // build our application with a route
    Router::new()
        // `GET /` goes to `root`
        .nest_service("/pkg", cargo_leptos_service.clone()) // Only need if using wasm-pack. Can be deleted if using cargo-leptos
        .nest_service(&bundle_path, cargo_leptos_service) // Only needed if using cargo-leptos. Can be deleted if using wasm-pack and cargo-run
        //.nest_service("/static", static_service)
        .leptos_routes(leptos_options.clone(), routes, |cx| view! { cx, <App/> })
}
