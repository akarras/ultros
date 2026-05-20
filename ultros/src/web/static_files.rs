//! Static-file serving for the web binary: the embedded/disk file lookup, plus the
//! `/favicon.ico`, `/robots.txt`, `/static/*` and `/item-icon/{id}` handlers.

use axum::body::{self, Body};
use axum::extract::{Path, Query};
use axum::http::{HeaderValue, Response, StatusCode};
use axum::response::IntoResponse;
use axum_extra::TypedHeader;
use axum_extra::headers::ContentType;
use hyper::header;
use serde::Deserialize;
use ultros_api_types::icon_size::IconSize;
use ultros_xiv_icons::get_item_image;

use crate::web::error::WebError;

/// In release mode, return the files from a statically included dir
#[cfg(not(debug_assertions))]
pub(crate) fn get_static_file(path: &str) -> Option<&'static [u8]> {
    use include_dir::include_dir;
    static STATIC_DIR: include_dir::Dir = include_dir!("$CARGO_MANIFEST_DIR/static");
    let dir = &STATIC_DIR;
    let file = dir.get_file(path)?;
    Some(file.contents())
}

/// In debug mode, just load the files from disk
#[cfg(debug_assertions)]
pub(crate) fn get_static_file(path: &str) -> Option<Vec<u8>> {
    use std::{io::Read, path::PathBuf};

    let file = PathBuf::from("./ultros/static").join(path);
    let mut file = std::fs::File::open(file).ok()?;
    let mut vec = Vec::new();
    file.read_to_end(&mut vec).ok()?;
    Some(vec)
}

pub(crate) async fn get_file(path: &str) -> Result<impl IntoResponse + use<>, WebError> {
    // Prevent path traversal attacks
    if path.contains("..") || path.starts_with('/') || path.starts_with('\\') {
        return Err(WebError::NotFound);
    }

    let mime_type = mime_guess::from_path(path).first_or_text_plain();
    match get_static_file(path) {
        None => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::new(http_body_util::Empty::new()))?),
        Some(file) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime_type.as_ref()).unwrap(),
            )
            .header(
                header::CACHE_CONTROL,
                #[cfg(not(debug_assertions))]
                HeaderValue::from_str("public, max-age=86400").unwrap(),
                #[cfg(debug_assertions)]
                HeaderValue::from_str("none").unwrap(),
            )
            .body(Body::new(http_body_util::Full::from(file)))?),
    }
}

pub(crate) async fn favicon() -> impl IntoResponse {
    get_file("favicon.ico").await
}

pub(crate) async fn robots() -> impl IntoResponse {
    get_file("robots.txt").await
}

pub(crate) async fn static_path(Path(path): Path<String>) -> impl IntoResponse {
    let path = path.trim_start_matches('/');
    get_file(path).await
}

/// Serve `ultros/static/service-worker.js` from the **root path** `/service-worker.js`.
///
/// A service worker's scope is restricted to (a subdirectory of) the path it's
/// served from. Serving it from `/static/service-worker.js` would only let it
/// control `/static/*`, which is useless. By placing it at `/service-worker.js`
/// — and explicitly opting in via `Service-Worker-Allowed: /` — we get site-wide
/// scope. Content-Type must be a JS MIME or browsers reject the registration.
pub(crate) async fn service_worker_js() -> Result<Response<Body>, WebError> {
    let bytes = get_static_file("service-worker.js").ok_or(WebError::NotFound)?;
    // In release mode `bytes` is `&'static [u8]` (embedded); in debug it's `Vec<u8>`
    // loaded from disk. Normalize to an owned Vec for the response body.
    #[cfg(not(debug_assertions))]
    let bytes: Vec<u8> = bytes.to_vec();
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/javascript"),
        )
        // Explicitly opt this SW into site-wide scope. Without this header the
        // browser would reject the registration when called from a non-root page.
        .header("Service-Worker-Allowed", HeaderValue::from_static("/"))
        // SW scripts should always come from the network so updates land quickly.
        .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
        .body(Body::new(http_body_util::Full::from(bytes)))?)
}

#[derive(Deserialize)]
pub(crate) struct IconQuery {
    pub size: IconSize,
}

pub(crate) async fn fallback_item_icon() -> impl IntoResponse {
    let fallback_image = include_bytes!("../../static/fallback-image.png");
    (TypedHeader(ContentType::png()), fallback_image)
}

pub(crate) async fn get_item_icon(
    Path(item_id): Path<u32>,
    Query(query): Query<IconQuery>,
) -> Result<Response<body::Body>, WebError> {
    // When an item has no icon (or the requested size variant is missing),
    // serve the static fallback PNG with 200 instead of throwing a 500.
    // Browsers render the placeholder cleanly and no console errors fire.
    if let Some(bytes) = get_item_image(item_id as i32, query.size) {
        let mime_type = mime_guess::from_path("icon.webp").first_or_text_plain();
        Ok(Response::builder()
            .header(
                header::CACHE_CONTROL,
                HeaderValue::from_static("max-age=86400"),
            )
            .header(header::CONTENT_TYPE, mime_type.as_ref())
            .body(body::Body::new(http_body_util::Full::from(bytes)))?)
    } else {
        let fallback: &'static [u8] = include_bytes!("../../static/fallback-image.png");
        Ok(Response::builder()
            .header(
                header::CACHE_CONTROL,
                HeaderValue::from_static("max-age=3600"),
            )
            .header(header::CONTENT_TYPE, "image/png")
            .body(body::Body::new(http_body_util::Full::from(fallback)))?)
    }
}
