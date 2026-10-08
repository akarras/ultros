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
        "<fallback>".to_owned()
    };
    let method = req.method().clone();

    let user_agent = req
        .headers()
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(user_agent_family)
        .unwrap_or("missing");
    let response = next.run(req).await;

    let latency = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    record_request(method.as_str(), &path, &status, user_agent, latency);

    response
}

fn record_request(method: &str, path: &str, status: &str, agent: &'static str, latency: f64) {
    let labels = [
        ("method", method.to_owned()),
        ("path", path.to_owned()),
        ("status", status.to_owned()),
    ];

    metrics::counter!("ultros_http_requests_total", &labels).increment(1);
    metrics::histogram!("ultros_http_requests_duration_seconds", &labels).record(latency);
    // Keep agent counts separate from route/status/latency series, and never
    // retain raw, client-controlled strings in the recorder or Prometheus.
    metrics::counter!("ultros_http_user_agents_total", "agent" => agent).increment(1);
}

fn user_agent_family(value: &str) -> &'static str {
    let value = value.to_ascii_lowercase();
    if value.is_empty() {
        "missing"
    } else if ["bot", "spider", "crawler"]
        .iter()
        .any(|s| value.contains(s))
    {
        "bot"
    } else if ["curl/", "wget/", "python", "go-http-client", "httpie/"]
        .iter()
        .any(|s| value.contains(s))
    {
        "tool"
    } else if value.contains("edg/") || value.contains("edgios/") || value.contains("edga/") {
        "edge"
    } else if value.contains("opr/") || value.contains("opera") {
        "opera"
    } else if value.contains("firefox/") || value.contains("fxios/") {
        "firefox"
    } else if value.contains("chrome/") || value.contains("crios/") {
        "chrome"
    } else if value.contains("safari/") {
        "safari"
    } else {
        "other"
    }
}

fn metrics_app(recorder_handle: PrometheusHandle) -> Router {
    Router::new().route("/metrics", get(move || ready(recorder_handle.render())))
}

/// Installs the global Prometheus recorder and returns the handle the
/// `/metrics` route renders from.
///
/// This must run in `main` *before any service is spawned*: the `metrics::`
/// macros silently no-op against the default `NoopRecorder` until a recorder
/// is installed, and several services emit samples during their own startup —
/// the analyzer's `ultros_analyzer_snapshot_rejected_total` /
/// `ultros_analyzer_snapshot_age_seconds` fire while restoring the snapshot,
/// long before `start_web` runs. Installing here and only *serving* from
/// `start_metrics_server` keeps startup ordering flexible without losing
/// those samples.
pub(crate) fn setup_metrics_recorder() -> PrometheusHandle {
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

pub(crate) async fn start_metrics_server(
    recorder_handle: PrometheusHandle,
    token: tokio_util::sync::CancellationToken,
) {
    let app = metrics_app(recorder_handle);

    // A separate port keeps metrics out of the public router. Allow parallel
    // local worktrees to choose their own listener while preserving production.
    let port = std::env::var("METRICS_PORT")
        .ok()
        .map(|port| {
            port.parse::<u16>()
                .expect("METRICS_PORT must be a valid port")
        })
        .unwrap_or(9091);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::debug!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(token.cancelled_owned())
        .await
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_versions_do_not_multiply_route_or_agent_series() {
        let recorder = PrometheusBuilder::new().build_recorder();
        metrics::with_local_recorder(&recorder, || {
            for version in 0..128 {
                let agent =
                    user_agent_family(&format!("Mozilla/5.0 Chrome/{version}.0 Safari/537.36"));
                record_request("GET", "/item/{id}", "200", agent, 0.01);
            }
        });
        let rendered = recorder.handle().render();
        let requests: Vec<_> = rendered
            .lines()
            .filter(|line| line.starts_with("ultros_http_requests_total{"))
            .collect();
        assert_eq!(requests.len(), 1, "{rendered}");
        assert!(requests[0].ends_with(" 128"), "{rendered}");
        for line in rendered
            .lines()
            .filter(|line| line.starts_with("ultros_http_requests_"))
        {
            assert!(!line.contains("agent="), "{line}");
        }
        let agents: Vec<_> = rendered
            .lines()
            .filter(|line| line.starts_with("ultros_http_user_agents_total{"))
            .collect();
        assert_eq!(
            agents,
            ["ultros_http_user_agents_total{agent=\"chrome\"} 128"]
        );
        assert!(!rendered.contains("Mozilla"));
    }

    #[test]
    fn agents_collapse_to_fixed_families_with_bot_and_browser_precedence() {
        for (value, expected) in [
            ("", "missing"),
            ("random client-controlled text", "other"),
            ("Mozilla Chrome/131 Safari/537 Googlebot/2.1", "bot"),
            ("Mozilla Chrome/131 Safari/537 Edg/131", "edge"),
            ("Mozilla Chrome/131 Safari/537 OPR/113", "opera"),
            ("Mozilla FxiOS/123 Safari/605", "firefox"),
            ("Mozilla CriOS/123 Safari/605", "chrome"),
            ("Mozilla Version/17 Safari/605", "safari"),
            ("CURL/8.0", "tool"),
        ] {
            assert_eq!(user_agent_family(value), expected, "{value}");
        }
    }
}
