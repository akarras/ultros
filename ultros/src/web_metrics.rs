use std::{future::ready, net::SocketAddr, time::Instant};

use axum::{
    Router, extract::MatchedPath, extract::Request, middleware::Next, response::IntoResponse,
    routing::get,
};
use hyper::header::USER_AGENT;
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

pub(crate) async fn track_metrics(req: Request, next: Next) -> impl IntoResponse {
    let start = Instant::now();
    let path = if let Some(matched_path) = req.extensions().get::<MatchedPath>() {
        matched_path.as_str().to_owned()
    } else {
        req.uri().path().to_owned()
    };
    let method = req.method().clone();

    let user_agent = req
        .headers()
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok().map(|s| s.to_string()))
        .unwrap_or_default();
    let response = next.run(req).await;

    let latency = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    let labels = [
        ("method", method.to_string()),
        ("path", path),
        ("status", status),
        ("user-agent", user_agent),
    ];

    metrics::counter!("ultros_http_requests_total", &labels).increment(1);
    metrics::histogram!("ultros_http_requests_duration_seconds", &labels).record(latency);

    response
}

fn metrics_app() -> Router {
    let recorder_handle = setup_metrics_recorder();
    Router::new().route("/metrics", get(move || ready(recorder_handle.render())))
}

fn setup_metrics_recorder() -> PrometheusHandle {
    const EXPONENTIAL_SECONDS: &[f64] = &[
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ];

    PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("ultros_http_requests_duration_seconds".to_string()),
            EXPONENTIAL_SECONDS,
        )
        .unwrap()
        .install_recorder()
        .unwrap()
}

pub(crate) async fn start_metrics_server() {
    let app = metrics_app();

    // NOTE: expose metrics enpoint on a different port
    let addr = SocketAddr::from(([0, 0, 0, 0], 9091));
    tracing::debug!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap()
}
