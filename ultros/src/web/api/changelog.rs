//! `/api/v1/changelog` — the release-notes history the changelog page renders.
//!
//! `ultros-changelog` compiles every change file into this binary, and the
//! wasm client deliberately builds without that table (see the crate's
//! `history` feature): a couple of hundred entries of prose is bundle weight
//! every visitor pays for one of the least-visited routes. So this is not a
//! query — it is a static read, and the changelog page's SSR resource
//! dispatches into it in-process on the first render.
//!
//! The list only changes on deploy, so it caches for a few minutes. That is
//! short enough that someone who follows the sidebar's what's-new dot
//! straight after a release still sees the entry the dot was pointing at —
//! the dot itself reads the compile-time date, not this response.

use std::time::Duration;

use axum::{Json, response::IntoResponse};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use ultros_changelog::CHANGELOG;

pub(crate) async fn get_changelog() -> impl IntoResponse {
    let mut response = Json(CHANGELOG).into_response();
    response.headers_mut().typed_insert(
        CacheControl::new()
            .with_public()
            .with_max_age(Duration::from_secs(300)),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{StatusCode, header::CACHE_CONTROL};

    /// The wasm client parses this body into `Vec<ChangelogEntry>`, so the
    /// borrowed server-side table has to round-trip through the wire format
    /// without losing an entry or its ordering.
    #[tokio::test]
    async fn serves_the_compiled_history_in_order_and_lets_it_be_cached() {
        let response = get_changelog().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let cache_control = response.headers()[CACHE_CONTROL].to_str().unwrap();
        assert!(cache_control.contains("public"), "{cache_control}");
        assert!(cache_control.contains("max-age="), "{cache_control}");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: Vec<ultros_changelog::ChangelogEntry> =
            serde_json::from_slice(&body).expect("the client must be able to parse this");
        assert_eq!(parsed, CHANGELOG);
    }
}
