/// Serves the jemalloc heap profile as pprof on port 3123.
///
/// The port is not published by the container, so it is only reachable from
/// the Docker network, e.g.
/// `docker run --rm --network <compose network> curlimages/curl -s http://ultros:3123/debug/pprof/heap > heap.pb.gz`.
/// Compare two dumps taken some time apart with
/// `go tool pprof -base heap1.pb.gz ultros heap2.pb.gz` to see what grew.
pub(crate) async fn start_profiling_server() {
    let app = axum::Router::new().route("/debug/pprof/heap", axum::routing::get(handle_get_heap));

    let listener = match tokio::net::TcpListener::bind("0.0.0.0:3123").await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::error!(%err, "heap profiling server failed to bind :3123");
            return;
        }
    };
    if let Err(err) = axum::serve(listener, app).await {
        tracing::error!(%err, "heap profiling server stopped");
    }
}

use axum::http::StatusCode;
use axum::response::IntoResponse;

pub async fn handle_get_heap() -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut prof_ctl = jemalloc_pprof::PROF_CTL.as_ref().unwrap().lock().await;
    require_profiling_activated(&prof_ctl)?;
    let pprof = prof_ctl
        .dump_pprof()
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(pprof)
}

/// Checks whether jemalloc profiling is activated an returns an error response if not.
fn require_profiling_activated(
    prof_ctl: &jemalloc_pprof::JemallocProfCtl,
) -> Result<(), (StatusCode, String)> {
    if prof_ctl.activated() {
        Ok(())
    } else {
        Err((
            axum::http::StatusCode::FORBIDDEN,
            "heap profiling not activated".into(),
        ))
    }
}
