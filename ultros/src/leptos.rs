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
use leptos::*;
use leptos_axum::generate_route_list;
use leptos_router::RouteListing;
#[cfg(not(debug_assertions))]
use tower_http::set_header::SetResponseHeader;
use tracing::instrument;
use ultros_api_types::world_helper::WorldHelper;
use ultros_app::*;

use crate::web::{country_code_decoder::Region, WebState};

#[instrument(skip(worlds, options, req))]
async fn custom_handler(
    State(worlds): State<Arc<WorldHelper>>,
    Extension(options): Extension<Arc<LeptosOptions>>,
    region: Option<Region>,
    req: Request<Body>,
) -> Response {
    let handler = leptos_axum::render_app_to_stream_with_context(
        (*options).clone(),
        move || {},
        move || view! { <App worlds=Ok(worlds.clone()) region=region.unwrap_or(Region::NorthAmerica).to_string()/> },
    );
    handler(req).await.into_response()
}

pub(crate) async fn create_leptos_app(
    worlds: Arc<WorldHelper>,
) -> Result<Router<WebState>, Box<dyn Error>> {
    use axum::http::StatusCode;
    use tower_http::services::ServeDir;

    let conf = get_configuration(None).await?;
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
    leptos_options.site_pkg_dir = ["pkg/", git_hash].concat();
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
        .leptos_routes_with_handler_stateful(routes, custom_handler)
        // .with_state(state)
        .layer(Extension(Arc::new(leptos_options))))
}

pub trait StatefulRoutes<S> {
    fn leptos_routes_with_handler_stateful<H, T>(
        self,
        paths: Vec<RouteListing>,
        handler: H,
    ) -> Self
    where
        H: axum::handler::Handler<T, S>,
        S: Clone + Send + Sync + 'static,
        T: 'static;
}

impl<S> StatefulRoutes<S> for axum::Router<S> {
    fn leptos_routes_with_handler_stateful<H, T>(self, paths: Vec<RouteListing>, handler: H) -> Self
    where
        H: axum::handler::Handler<T, S>,
        S: Clone + Send + Sync + 'static,
        T: 'static,
    {
        let mut router = self;
        for route in paths.iter() {
            router = router.route(route.path(), get(handler.clone()));
        }
        router
    }
}

#[cfg(test)]
mod test {
    use super::StatefulRoutes;
    use axum::{
        async_trait,
        extract::{FromRef, FromRequestParts, State},
        http::request::Parts,
        Router,
    };
    use axum_macros::debug_handler;
    use std::sync::Arc;

    #[test]
    fn test_add_handler_without_state() {
        #[debug_handler]
        async fn handler() -> String {
            "Hello world".to_string()
        }

        let _ = Router::<()>::new().leptos_routes_with_handler_stateful(vec![], handler);
    }

    #[test]
    fn test_add_handler_with_state() {
        #[derive(Clone, Debug)]
        struct AppState(Arc<String>);
        #[debug_handler]
        async fn handler(State(state): State<AppState>) -> String {
            state.0.to_string()
        }
        let state = AppState(Arc::new("Hello world".to_string()));
        let _router = Router::<AppState>::new()
            .leptos_routes_with_handler_stateful(vec![], handler)
            .with_state::<AppState>(state);
    }

    #[test]
    fn test_handler_with_ref_state() {
        #[derive(Clone, Copy, Debug)]
        struct AState();

        #[derive(Clone, Copy, Debug)]
        struct OtherState();

        #[derive(Clone, Debug)]
        struct WebState {
            a_state: AState,
            other: OtherState,
        }

        impl FromRef<WebState> for OtherState {
            fn from_ref(input: &WebState) -> Self {
                input.other.clone()
            }
        }

        impl FromRef<WebState> for AState {
            fn from_ref(input: &WebState) -> Self {
                input.a_state
            }
        }

        // Wrapper that returns AState directly as a request part
        struct InnerData();

        #[async_trait]
        impl<S> FromRequestParts<S> for InnerData
        where
            AState: FromRef<S>,
            S: Send + Sync,
        {
            type Rejection = ();
            async fn from_request_parts(
                parts: &mut Parts,
                state: &S,
            ) -> Result<Self, Self::Rejection> {
                let State(_) = <State<AState>>::from_request_parts(parts, state)
                    .await
                    .map_err(|_| ())?;

                Ok(Self())
            }
        }

        #[debug_handler]
        async fn handler(State(_other_state): State<WebState>, _data: InnerData) {}

        let _ = Router::<WebState>::new()
            .leptos_routes_with_handler_stateful(vec![], handler)
            .with_state::<WebState>(WebState {
                a_state: AState(),
                other: OtherState(),
            });
    }
}
