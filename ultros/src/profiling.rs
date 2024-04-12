pub(crate) async fn start_profiling_server() {
    let mut v = vec![];
    for i in 0..1000000 {
        v.push(i);
    }

    let app = axum::Router::new().route("/debug/pprof/heap", axum::routing::get(handle_get_heap));

    // run our app with hyper, listening globally on port 3123
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3123").await.unwrap();
    axum::serve(listener, app).await.unwrap();
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
