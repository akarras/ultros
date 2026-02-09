use axum::body::Body;
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
fn get_static_file(path: &str) -> Option<&'static [u8]> {
    use include_dir::include_dir;
    static STATIC_DIR: include_dir::Dir = include_dir!("$CARGO_MANIFEST_DIR/static");
    let dir = &STATIC_DIR;
    let file = dir.get_file(path)?;
    Some(file.contents())
}

/// In debug mode, just load the files from disk
#[cfg(debug_assertions)]
fn get_static_file(path: &str) -> Option<Vec<u8>> {
    use std::{io::Read, path::PathBuf};

    let file = PathBuf::from("./ultros/static").join(path);
    let mut file = std::fs::File::open(file).ok()?;
    let mut vec = Vec::new();
    file.read_to_end(&mut vec).ok()?;
    Some(vec)
}

async fn get_file(path: &str) -> Result<impl IntoResponse + use<>, WebError> {
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

#[derive(Deserialize)]
pub(crate) struct IconQuery {
    size: IconSize,
}

pub(crate) async fn fallback_item_icon() -> impl IntoResponse {
    let fallback_image = include_bytes!("../../static/fallback-image.png");
    (TypedHeader(ContentType::png()), fallback_image)
}

pub(crate) async fn get_item_icon(
    Path(item_id): Path<u32>,
    Query(query): Query<IconQuery>,
) -> Result<impl IntoResponse, WebError> {
    let bytes =
        get_item_image(item_id as i32, query.size).ok_or(anyhow::anyhow!("Failed to get icon"))?;
    let mime_type = mime_guess::from_path("icon.webp").first_or_text_plain();
    let age_header = HeaderValue::from_static("max-age=86400");
    Ok(Response::builder()
        .header(header::CACHE_CONTROL, age_header)
        .header(header::CONTENT_TYPE, mime_type.as_ref())
        .body(Body::new(http_body_util::Full::from(bytes)))?)
}

pub(crate) async fn get_bincode() -> &'static [u8] {
    xiv_gen_db::bincode()
}
