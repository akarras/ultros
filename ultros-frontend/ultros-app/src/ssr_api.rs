//! In-process transport for the same Axum API that the browser calls.
//!
//! Provide this router in the render's Leptos context. It must contain only
//! API routes and their application middleware, without the page fallback,
//! host redirects, or HTTP compression. Request headers are supplied per call
//! from the current render, never stored in the shared router.

use std::time::Duration;

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use tower::ServiceExt;
use tracing::Instrument;

use crate::error::{AppError, AppResult};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const URI_ENCODE_SET: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`');

#[derive(Clone, Debug)]
pub struct SsrApi(Router);

impl SsrApi {
    pub fn new(router: Router) -> Self {
        Self(router)
    }

    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
        headers: HeaderMap,
        json: Option<String>,
    ) -> AppResult<(StatusCode, Bytes)> {
        self.request_with_timeout(method, path, headers, json, REQUEST_TIMEOUT)
            .await
    }

    async fn request_with_timeout(
        &self,
        method: Method,
        path: &str,
        headers: HeaderMap,
        json: Option<String>,
        timeout: Duration,
    ) -> AppResult<(StatusCode, Bytes)> {
        let mut headers = request_headers(headers);
        let body = if let Some(json) = json {
            headers.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/json"),
            );
            headers.insert(header::CONTENT_LENGTH, json.len().into());
            Body::from(json)
        } else {
            Body::empty()
        };
        // Build fresh parts: page Path/MatchedPath/OriginalUri extensions must
        // not leak into the new API route's extractors.
        // reqwest previously encoded Unicode names and spaces for us. Axum's
        // URI type requires ASCII; preserve existing escapes and URL delimiters.
        let path = percent_encoding::utf8_percent_encode(path, URI_ENCODE_SET).to_string();
        let mut request = Request::builder()
            .method(method.clone())
            .uri(&path)
            .body(body)
            .map_err(anyhow::Error::from)?;
        *request.headers_mut() = headers;

        tokio::time::timeout(
            timeout,
            async {
                let response = self.0.clone().oneshot(request).await.unwrap();
                let status = response.status();
                // As with the old HTTP transport, internal response headers (e.g.
                // Set-Cookie) are not applied to the enclosing HTML response.
                // Include body collection in the deadline, not only handler execution.
                let body = to_bytes(response.into_body(), usize::MAX)
                    .await
                    .map_err(anyhow::Error::from)?;
                Ok((status, body))
            }
            .instrument(tracing::info_span!("ssr_api", %method, %path)),
        )
        .await
        .map_err(|_| AppError::InternalApiTimeout)?
    }
}

fn request_headers(mut headers: HeaderMap) -> HeaderMap {
    // Retain application headers verbatim, including repeated values, Host,
    // cookies, locale, and the original proxy metadata. No new network hop is
    // made. Only transport metadata and the old body's framing are discarded.
    let connection_headers: Vec<_> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| header::HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect();
    for name in connection_headers {
        headers.remove(name);
    }
    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "content-length",
        "content-type",
        "content-encoding",
        "accept-encoding",
    ] {
        headers.remove(name);
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json,
        extract::{Path, Query, State},
        routing::{get, post},
    };
    use serde_json::{Value, json};
    use std::collections::HashMap;

    fn visitor(cookie: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in [
            ("cookie", cookie),
            ("host", "ultros.app"),
            ("authorization", "Bearer example"),
            ("accept-language", "ja-JP"),
            ("accept-language", "en-US"),
            ("cf-ipcountry", "JP"),
            ("x-forwarded-for", "192.0.2.1"),
            ("x-custom", "one"),
            ("x-custom", "two"),
            ("connection", "keep-alive, x-hop"),
            ("x-hop", "connection-specific"),
            ("content-length", "999"),
            ("content-type", "text/html"),
            ("accept-encoding", "br, gzip"),
        ] {
            headers.append(
                header::HeaderName::from_static(name),
                header::HeaderValue::from_static(value),
            );
        }
        headers
    }

    async fn inspect(
        State(prefix): State<&'static str>,
        Path(name): Path<String>,
        Query(query): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Json<Value> {
        tokio::task::yield_now().await;
        assert_eq!(headers["host"], "ultros.app");
        assert_eq!(headers["authorization"], "Bearer example");
        assert_eq!(headers["cf-ipcountry"], "JP");
        assert_eq!(headers["x-forwarded-for"], "192.0.2.1");
        assert_eq!(
            headers
                .get_all("accept-language")
                .iter()
                .collect::<Vec<_>>(),
            ["ja-JP", "en-US"]
        );
        assert_eq!(
            headers.get_all("x-custom").iter().collect::<Vec<_>>(),
            ["one", "two"]
        );
        for removed in [
            "connection",
            "x-hop",
            "content-length",
            "content-type",
            "content-encoding",
            "accept-encoding",
        ] {
            assert!(!headers.contains_key(removed), "{removed}");
        }
        Json(
            json!({"name": name, "query": query["q"], "cookie": headers["cookie"].to_str().unwrap(), "state": prefix}),
        )
    }

    #[tokio::test]
    async fn routes_extract_original_headers_and_keep_concurrent_visitors_separate() {
        let api = SsrApi::new(
            Router::new()
                .route("/api/v1/inspect/{name}", get(inspect))
                .with_state("shared"),
        );
        let (alice, bob) = tokio::join!(
            api.request(
                Method::GET,
                "/api/v1/inspect/日本 Name?q=a%2Fb",
                visitor("session=alice"),
                None
            ),
            api.request(
                Method::GET,
                "/api/v1/inspect/日本 Name?q=a%2Fb",
                visitor("session=bob"),
                None
            ),
        );
        for (result, cookie) in [(alice, "session=alice"), (bob, "session=bob")] {
            let (status, body) = result.unwrap();
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap(),
                json!({"name": "日本 Name", "query": "a/b", "cookie": cookie, "state": "shared"})
            );
        }
    }

    #[tokio::test]
    async fn mutations_use_the_same_router_and_replace_body_headers() {
        async fn echo(headers: HeaderMap, Json(value): Json<Value>) -> Json<Value> {
            assert_eq!(headers["cookie"], "session=alice");
            assert_eq!(headers["content-type"], "application/json");
            assert_eq!(headers["content-length"], "14");
            Json(value)
        }
        let api =
            SsrApi::new(Router::new().route("/api/v1/echo", post(echo).patch(echo).delete(echo)));
        for method in [Method::POST, Method::PATCH, Method::DELETE] {
            let (status, body) = api
                .request(
                    method,
                    "/api/v1/echo",
                    visitor("session=alice"),
                    Some(r#"{"value":1234}"#.into()),
                )
                .await
                .unwrap();
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap(),
                json!({"value":1234})
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_covers_both_handler_and_streaming_body() {
        let api = SsrApi::new(
            Router::new()
                .route(
                    "/handler",
                    get(|| async { std::future::pending::<String>().await }),
                )
                .route(
                    "/body",
                    get(|| async {
                        Body::from_stream(
                            futures::stream::pending::<Result<Bytes, std::io::Error>>(),
                        )
                    }),
                ),
        );
        for path in ["/handler", "/body"] {
            let error = api
                .request_with_timeout(
                    Method::GET,
                    path,
                    HeaderMap::new(),
                    None,
                    Duration::from_millis(10),
                )
                .await
                .unwrap_err();
            assert_eq!(error, AppError::InternalApiTimeout);
        }
    }

    #[tokio::test]
    async fn api_only_router_preserves_rejections_and_does_not_render_unknown_paths() {
        let api = SsrApi::new(Router::new().route(
            "/api/v1/private",
            get(|headers: HeaderMap| async move {
                if headers.contains_key(header::COOKIE) {
                    StatusCode::FORBIDDEN
                } else {
                    StatusCode::UNAUTHORIZED
                }
            }),
        ));
        for (path, headers, expected) in [
            (
                "/api/v1/private",
                HeaderMap::new(),
                StatusCode::UNAUTHORIZED,
            ),
            (
                "/api/v1/private",
                visitor("session=alice"),
                StatusCode::FORBIDDEN,
            ),
            ("/unknown-page", HeaderMap::new(), StatusCode::NOT_FOUND),
        ] {
            assert_eq!(
                api.request(Method::GET, path, headers, None)
                    .await
                    .unwrap()
                    .0,
                expected
            );
        }
    }
}
