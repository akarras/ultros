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
    let plugin_page = crate::web::plugin::browser::is_page(req.uri().path());
    if plugin_page && !crate::web::plugin::browser::valid_page_request(&req) {
        let mut response = axum::http::StatusCode::FORBIDDEN.into_response();
        crate::web::plugin::browser::page_headers(&mut response);
        return response;
    }
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
        lists_readonly: bool,
    }
    let lists_readonly = ultros_server_core::lists_readonly::lists_readonly();
    let bootstrap_json = serde_json::to_string(&BootstrapRef {
        world_data: worlds.world_data(),
        region: &region_str,
        current_user: &current_user,
        lists_readonly,
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
        provide_context(ListsReadonly(lists_readonly));
        provide_context(api.clone());
    };
    let app_fn = move || shell(options.clone(), bootstrap_script.clone(), plugin_page);
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
    if plugin_page {
        crate::web::plugin::browser::page_headers(&mut response);
    }
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

/// Mounts `/pkg/<git_hash>/{*file}` against `bundle_dir` and, when
/// `archive_dir` is provided, also mounts `/pkg/{hash}/{*file}` so rolling N-1
/// bundles remain servable across deploys.
fn mount_pkg_routes<S>(
    mut router: Router<S>,
    bundle_dir: std::path::PathBuf,
    git_hash: &str,
    archive_dir: Option<Arc<std::path::PathBuf>>,
) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let local_bundle = Arc::new(bundle_dir);
    router = router.route(
        &format!("/pkg/{git_hash}/{{*file}}"),
        axum::routing::get(
            move |axum::extract::Path(file): axum::extract::Path<String>,
                  req: axum::extract::Request| {
                let bundle = local_bundle.clone();
                async move { crate::pkg_archive::serve_local_pkg(&bundle, &file, req).await }
            },
        ),
    );
    if let Some(archive_dir) = archive_dir {
        router = router.route(
            "/pkg/{hash}/{*file}",
            axum::routing::get(
                move |axum::extract::Path((hash, file)): axum::extract::Path<(String, String)>,
                      req: axum::extract::Request| {
                    let archive_dir = archive_dir.clone();
                    async move {
                        crate::pkg_archive::serve_archived_pkg(&archive_dir, &hash, &file, req)
                            .await
                    }
                },
            ),
        );
    }
    router
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
    tracing::info!("Serving pkg dir: {bundle_filepath}");
    let pkg_archive_dir = crate::pkg_archive::PkgArchiveConfig::from_env().map(|cfg| {
        if let Err(error) = crate::pkg_archive::sync_pkg_archive(
            std::path::Path::new(&bundle_filepath),
            &cfg.archive_dir,
            git_hash,
            cfg.retain,
        ) {
            tracing::warn!(
                ?error,
                archive = %cfg.archive_dir.display(),
                "failed to sync current pkg bundle into N-1 archive; continuing with local bundle"
            );
        }
        Arc::new(cfg.archive_dir)
    });
    let worlds = Ok(worlds);
    let routes = generate_route_list(move || {
        let worlds = worlds.clone();
        provide_context(LocalWorldData(worlds));
        provide_context(GuessedRegion("North-America".to_string()));
        provide_context(BootstrapUser(None));
        provide_context(ListsReadonly(false));
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
    let mut router = mount_pkg_routes(
        Router::new()
            // Axum's generated route list omits the empty child's trailing slash.
            // Serve the guide through the same protected Leptos handler either way.
            .route("/plugin/", axum::routing::get(in_order_handler)),
        std::path::PathBuf::from(&bundle_filepath),
        git_hash,
        pkg_archive_dir,
    );
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
    use super::{escape_for_script_tag, mount_pkg_routes};
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use http_body_util::BodyExt;
    use std::sync::Arc;
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
        get_pkg_path(dir, "/ultros.wasm", accept_encoding).await
    }

    async fn get_pkg_path(
        dir: &tempfile::TempDir,
        uri: &str,
        accept_encoding: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let mut request = Request::builder().uri(uri);
        if let Some(accept_encoding) = accept_encoding {
            request = request.header(header::ACCEPT_ENCODING, accept_encoding);
        }
        let response =
            crate::pkg_archive::serve_pkg_file(dir.path(), request.body(Body::empty()).unwrap())
                .await;
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

    #[tokio::test]
    async fn pkg_missing_file_returns_no_store_not_immutable() {
        let dir = precompressed_pkg_dir();
        let (status, headers, _) = get_pkg_path(&dir, "/does-not-exist.wasm", Some("br")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    }

    #[tokio::test]
    async fn mounted_pkg_router_prefers_local_bundle_and_falls_back_to_n_minus_1_archive() {
        let root = tempfile::tempdir().unwrap();
        let local_bundle = root.path().join("local");
        let old_bundle = root.path().join("old");
        let archive_dir = root.path().join("archive");
        std::fs::create_dir_all(&local_bundle).unwrap();
        std::fs::create_dir_all(&old_bundle).unwrap();
        std::fs::write(local_bundle.join("ultros.wasm"), b"local-current").unwrap();
        std::fs::write(old_bundle.join("ultros.wasm"), b"archived-old").unwrap();
        std::fs::write(old_bundle.join("ultros.wasm.br"), b"archived-old-br").unwrap();

        crate::pkg_archive::sync_pkg_archive(&old_bundle, &archive_dir, "oldhash", 3).unwrap();
        crate::pkg_archive::sync_pkg_archive(&local_bundle, &archive_dir, "newhash", 3).unwrap();
        // Mutate the archived copy of `newhash` to prove requests for the
        // running hash serve directly from the local container bundle.
        std::fs::write(
            archive_dir.join("newhash").join("ultros.wasm"),
            b"archive-copy",
        )
        .unwrap();

        let app: Router<()> = mount_pkg_routes(
            Router::new(),
            local_bundle,
            "newhash",
            Some(Arc::new(archive_dir.clone())),
        );

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/pkg/newhash/ultros.wasm")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"local-current");

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/pkg/oldhash/ultros.wasm")
                    .header(header::ACCEPT_ENCODING, "br")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers()[header::CONTENT_ENCODING], "br");
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"archived-old-br");

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/pkg/expiredhash/ultros.wasm")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(resp.headers()[header::CACHE_CONTROL], "no-store");

        // Pre-publishing a future N+1 hash into the shared archive makes it
        // immediately servable by the already-running N router during cutover.
        let next_bundle = root.path().join("next");
        std::fs::create_dir_all(&next_bundle).unwrap();
        std::fs::write(next_bundle.join("ultros.wasm"), b"prepublished-next").unwrap();
        crate::pkg_archive::sync_pkg_archive(&next_bundle, &archive_dir, "nexthash", 3).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/pkg/nexthash/ultros.wasm")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"prepublished-next");

        // Reverse direction for the same N / N+1 pair: mount an N+1 router
        // against the same shared archive and verify it serves its own local
        // bundle for N+1 and falls back to the archive for N.
        let app_next: Router<()> = mount_pkg_routes(
            Router::new(),
            next_bundle,
            "nexthash",
            Some(Arc::new(archive_dir)),
        );
        let resp = app_next
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/pkg/nexthash/ultros.wasm")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"prepublished-next");

        let resp = app_next
            .oneshot(
                Request::builder()
                    .uri("/pkg/newhash/ultros.wasm")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"archive-copy");

        // Encoded and double-encoded hidden-file / traversal attempts must be
        // rejected on both the local and archived routes with `no-store`.
        for bad_uri in [
            "/pkg/oldhash/.published",
            "/pkg/oldhash/%2epublished",
            "/pkg/oldhash/%252epublished",
            "/pkg/oldhash/%2e%2e/newhash/ultros.wasm",
            "/pkg/newhash/%252epublished",
            "/pkg/newhash/ultros.wasm%3ffoo",
        ] {
            let resp = app
                .clone()
                .oneshot(Request::builder().uri(bad_uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "URI {bad_uri} must be rejected"
            );
            assert_eq!(resp.headers()[header::CACHE_CONTROL], "no-store");
        }
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
