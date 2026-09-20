use std::{error::Error, sync::Arc};

use axum::http::{HeaderValue, header};
/// Ultros UI server contains all the axum routes required to serve and bundle leptos wasm files
/// # Building
/// I recommend you use cargo-leptos, once you go through the steps to install cargo-leptos
/// you should be able to build and serve leptos with one install step.
///
use axum::{
    Extension, Router,
    body::Body,
    extract::State,
    http::Request,
    response::{IntoResponse, Response},
};
use leptos::prelude::*;
use leptos_axum::{LeptosRoutes, generate_route_list};
use leptos_router::SsrMode;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeader;
use tracing::{info, instrument};
use ultros_api_types::user::UserData;
use ultros_api_types::world_helper::WorldHelper;
use ultros_app::ssr_api::SsrApi;
use ultros_app::*;

use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;
use crate::web::{WebState, country_code_decoder::Region};
use ultros_app::script_escape::escape_for_script_tag;

/// Which streaming strategy to render a route with.
///
/// `leptos_routes_with_handler` binds a single handler to every route and never
/// consults the `SsrMode` a route declares, so the mode has to be threaded
/// through by hand — see `create_leptos_app`.
#[derive(Clone, Copy, Debug)]
enum StreamMode {
    OutOfOrder,
    InOrder,
}

async fn render_leptos(
    worlds: Arc<WorldHelper>,
    options: LeptosOptions,
    region: Option<Region>,
    user: Result<AuthDiscordUser, ApiError>,
    req: Request<Body>,
    mode: StreamMode,
    api: SsrApi,
) -> Response {
    info!("Custom handler");
    // The HTML now carries per-user data (region + current_user), so it must
    // never be cached by a shared proxy.
    let region_str = region.unwrap_or(Region::NorthAmerica).to_string();
    let current_user = user.ok().map(|u| UserData {
        id: u.id,
        username: u.name,
        avatar: u.avatar_url,
    });

    // Build the bootstrap script body once per request. We serialize a borrowed
    // view of WorldData to avoid cloning the (small but non-trivial) world tree.
    #[derive(serde::Serialize)]
    struct BootstrapRef<'a> {
        world_data: &'a ultros_api_types::world::WorldData,
        region: &'a str,
        current_user: &'a Option<UserData>,
    }
    let bootstrap_json = serde_json::to_string(&BootstrapRef {
        world_data: worlds.world_data(),
        region: &region_str,
        current_user: &current_user,
    })
    .unwrap_or_else(|_| "null".to_string());
    let bootstrap_script = format!(
        "window.__ULTROS_BOOTSTRAP__={};",
        escape_for_script_tag(&bootstrap_json)
    );

    let region_for_ctx = region_str.clone();
    let current_user_for_ctx = current_user.clone();
    let additional_context = move || {
        provide_context(LocalWorldData(Ok(worlds.clone())));
        provide_context(GuessedRegion(region_for_ctx.clone()));
        provide_context(BootstrapUser(current_user_for_ctx.clone()));
        provide_context(api.clone());
    };
    let app_fn = move || shell(options.clone(), bootstrap_script.clone());
    let mut response = match mode {
        StreamMode::OutOfOrder => {
            let handler = leptos_axum::render_app_to_stream_with_context_and_replace_blocks(
                additional_context,
                app_fn,
                true,
            );
            handler(req).await.into_response()
        }
        StreamMode::InOrder => {
            let handler =
                leptos_axum::render_app_to_stream_in_order_with_context(additional_context, app_fn);
            handler(req).await.into_response()
        }
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

#[instrument(skip(worlds, options, req, user, api))]
#[axum::debug_handler(state = WebState)]
async fn custom_handler(
    State(worlds): State<Arc<WorldHelper>>,
    State(options): State<LeptosOptions>,
    Extension(api): Extension<SsrApi>,
    region: Option<Region>,
    user: Result<AuthDiscordUser, ApiError>,
    req: Request<Body>,
) -> Response {
    // Detached so a client that leaves mid-render cannot cancel the render
    // and tear the reactive owner down under leptos' still-running Suspense
    // tasks — see `ssr_drain`.
    crate::ssr_drain::detach_render(render_leptos(
        worlds,
        options,
        region,
        user,
        req,
        StreamMode::OutOfOrder,
        api,
    ))
    .await
}

/// Handler for routes that declare `SsrMode::InOrder`.
///
/// Out-of-order streaming emits each resolved `<Suspense>` as a `<template>`
/// plus an inline script that relocates the fragment into place. Those
/// relocations race `hydrate_body()` and desync tachys' hydration walk, which
/// panics at `hydration.rs:163` (`failed_to_cast_element`) — GlitchTip #6831.
/// In-order streaming resolves each boundary before emitting it, so no
/// relocation script is produced and there is nothing to race.
#[instrument(skip(worlds, options, req, user, api))]
#[axum::debug_handler(state = WebState)]
async fn in_order_handler(
    State(worlds): State<Arc<WorldHelper>>,
    State(options): State<LeptosOptions>,
    Extension(api): Extension<SsrApi>,
    region: Option<Region>,
    user: Result<AuthDiscordUser, ApiError>,
    req: Request<Body>,
) -> Response {
    crate::ssr_drain::detach_render(render_leptos(
        worlds,
        options,
        region,
        user,
        req,
        StreamMode::InOrder,
        api,
    ))
    .await
}

/// The file service behind `/pkg/<git hash>/`: cargo-leptos's JS, wasm and CSS.
type PkgService = SetResponseHeader<SetResponseHeader<ServeDir, HeaderValue>, HeaderValue>;

/// Serves the cargo-leptos output directory.
///
/// Release builds run `cargo leptos build --precompress` (see the Dockerfile),
/// which writes a brotli `-q 11` `.br` and a gzip `-9` `.gz` sibling next to
/// every file. `ServeDir` picks the best of those the client accepts and sets
/// `Content-Encoding` itself; the outer `CompressionLayer` leaves responses that
/// already carry that header alone. Before this, the layer compressed the wasm
/// on every cold request at tower-http's default quality — 7.2 MB on the wire
/// where `-q 11` of the same file is 4.9 MB. Without siblings on disk (dev
/// builds) the plain file is served and the layer compresses it as before.
///
/// `Vary: accept-encoding` is appended because `ServeDir` does not add it for
/// precompressed responses, and the `/pkg/` URLs are cached for a year.
fn pkg_service(dir: impl AsRef<std::path::Path>) -> PkgService {
    let files = ServeDir::new(dir).precompressed_br().precompressed_gzip();
    let files = SetResponseHeader::appending(
        files,
        header::VARY,
        HeaderValue::from_static("accept-encoding"),
    );
    // The pkg dir is namespaced by GIT_HASH, so these URLs change on every
    // deploy and their contents never do — a one-day max-age just forced
    // needless revalidation. One year is the `immutable` ceiling. Dev builds
    // ask for revalidation instead so `cargo leptos watch` never serves stale
    // output.
    let cache_control = if cfg!(debug_assertions) {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    };
    SetResponseHeader::appending(
        files,
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    )
}

pub(crate) async fn create_leptos_app(
    worlds: Arc<WorldHelper>,
    api: SsrApi,
) -> Result<Router<WebState>, Box<dyn Error>> {
    let conf = get_configuration(None)?;
    let mut leptos_options = conf.leptos_options;
    let site_root = &leptos_options.site_root;
    let pkg_dir = &leptos_options.site_pkg_dir;

    // The URL path of the generated JS/WASM bundle from cargo-leptos
    // let bundle_path = format!("/{site_root}/{pkg_dir}");
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
    let git_hash = env!("GIT_HASH");
    leptos_options.site_pkg_dir = Arc::from(["pkg/", git_hash].concat());
    let cargo_leptos_service = pkg_service(&bundle_filepath);
    tracing::info!("Serving pkg dir: {bundle_filepath}");
    let worlds = Ok(worlds);
    let routes = generate_route_list(move || {
        let worlds = worlds.clone();
        provide_context(LocalWorldData(worlds));
        provide_context(GuessedRegion("North-America".to_string()));
        provide_context(BootstrapUser(None));
        view! { <App /> }
    });

    // simple_logger::init_with_level(log::Level::Debug).expect("couldn't initialize logging");

    // `leptos_routes_with_handler` binds one handler to every route and drops
    // the `SsrMode` each route declares — so `ssr=SsrMode::InOrder` in the app's
    // route table was silently ignored, and the item page kept streaming
    // out-of-order (#6831). Split the listing by mode and give the in-order
    // routes a handler that actually streams in order.
    let (in_order_routes, streaming_routes): (Vec<_>, Vec<_>) = routes
        .into_iter()
        .partition(|route| matches!(route.mode(), SsrMode::InOrder));
    tracing::info!(
        "leptos routes: {} in-order, {} out-of-order",
        in_order_routes.len(),
        streaming_routes.len()
    );

    // build our application with a route
    let mut router = Router::new()
        // `GET /` goes to `root`
        .nest_service(
            &["/", &leptos_options.site_pkg_dir].concat(),
            cargo_leptos_service.clone(),
        ); // Only need if using wasm-pack. Can be deleted if using cargo-leptos
    // .nest_service(&bundle_path, cargo_leptos_service) // Only needed if using cargo-leptos. Can be deleted if using wasm-pack and cargo-run
    //.nest_service("/static", static_service)
    if !in_order_routes.is_empty() {
        router = router.leptos_routes_with_handler(in_order_routes, in_order_handler);
    }
    Ok(router
        .leptos_routes_with_handler(streaming_routes, custom_handler)
        .layer(Extension(api)))
    // .with_state(state)
    // .layer(Extension(Arc::new(leptos_options))))
}

#[cfg(test)]
mod tests {
    use super::{escape_for_script_tag, pkg_service};
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// A pkg dir the way `cargo leptos build --precompress` leaves it: the
    /// wasm plus a `.br` and a `.gz` sibling with distinguishable contents.
    fn precompressed_pkg_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("ultros.wasm"), b"plain-wasm").unwrap();
        std::fs::write(dir.path().join("ultros.wasm.br"), b"brotli-bytes").unwrap();
        std::fs::write(dir.path().join("ultros.wasm.gz"), b"gzip-bytes").unwrap();
        dir
    }

    async fn get_wasm(
        dir: &tempfile::TempDir,
        accept_encoding: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let mut request = Request::builder().uri("/ultros.wasm");
        if let Some(accept_encoding) = accept_encoding {
            request = request.header(header::ACCEPT_ENCODING, accept_encoding);
        }
        let response = pkg_service(dir.path())
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (parts, body) = response.into_parts();
        let body = body.collect().await.unwrap().to_bytes().to_vec();
        (parts.status, parts.headers, body)
    }

    #[tokio::test]
    async fn pkg_serves_the_precompressed_brotli_sibling() {
        let dir = precompressed_pkg_dir();
        let (status, headers, body) = get_wasm(&dir, Some("gzip, deflate, br")).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, b"brotli-bytes");
        assert_eq!(headers[header::CONTENT_ENCODING], "br");
        // The encoding must not change the advertised type of the file.
        assert_eq!(headers[header::CONTENT_TYPE], "application/wasm");
        assert!(
            headers
                .get_all(header::VARY)
                .iter()
                .any(|v| v.to_str().unwrap().eq_ignore_ascii_case("accept-encoding")),
            "a precompressed response cached for a year must vary on Accept-Encoding"
        );
        assert!(headers.contains_key(header::CACHE_CONTROL));
    }

    #[tokio::test]
    async fn pkg_falls_back_to_gzip_then_plain() {
        let dir = precompressed_pkg_dir();

        let (_, headers, body) = get_wasm(&dir, Some("gzip")).await;
        assert_eq!(body, b"gzip-bytes");
        assert_eq!(headers[header::CONTENT_ENCODING], "gzip");

        let (_, headers, body) = get_wasm(&dir, None).await;
        assert_eq!(body, b"plain-wasm");
        assert!(!headers.contains_key(header::CONTENT_ENCODING));
    }

    #[tokio::test]
    async fn pkg_without_siblings_serves_the_plain_file() {
        // Dev builds never run --precompress; the plain file must still serve
        // so the outer CompressionLayer can compress it on the fly.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("ultros.wasm"), b"plain-wasm").unwrap();

        let (status, headers, body) = get_wasm(&dir, Some("br")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, b"plain-wasm");
        assert!(!headers.contains_key(header::CONTENT_ENCODING));
    }

    #[test]
    fn script_bootstrap_json_cannot_close_script_tag() {
        let payload = format!(
            r#"{{"name":"</script><script>alert(1)</script>&{}{}"}}"#,
            '\u{2028}', '\u{2029}'
        );
        let escaped = escape_for_script_tag(&payload);

        assert!(!escaped.contains("</script>"));
        assert!(escaped.contains("\\u003c/script\\u003e"));
        assert!(escaped.contains("\\u0026"));
        assert!(escaped.contains("\\u2028"));
        assert!(escaped.contains("\\u2029"));
    }
}
