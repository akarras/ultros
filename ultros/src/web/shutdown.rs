//! Cooperative shutdown for long-lived request handlers.
//!
//! `axum::serve(..).with_graceful_shutdown(..)` stops accepting new
//! connections when the shutdown future resolves, but it still waits for every
//! connection that is already in flight to finish. A handler that parks
//! forever — a websocket loop, a long poll — therefore keeps the whole server
//! future pending, and `main`'s 30 second shutdown budget expires with
//! "Graceful shutdown exceeded 30 seconds" (GlitchTip #7310).
//!
//! Handlers like that have to watch the same [`CancellationToken`] the serve
//! loop watches. [`until_shutdown`] is the wrapper for doing that at the one
//! `.await` that would otherwise block.

use std::future::Future;

use tokio_util::sync::CancellationToken;

/// Awaits `fut`, unless `token` is cancelled first.
///
/// Returns `Some(output)` when `fut` finished, `None` when the server is
/// shutting down and the caller should close the connection and return.
///
/// Biased toward the token: once shutdown has started a busy socket must not
/// be able to starve it.
pub(crate) async fn until_shutdown<F>(token: &CancellationToken, fut: F) -> Option<F::Output>
where
    F: Future,
{
    tokio::select! {
        biased;
        _ = token.cancelled() => None,
        output = fut => Some(output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Duration;

    use axum::Router;
    use axum::routing::get;
    use tokio::net::TcpListener;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn passes_the_future_output_through_when_not_cancelled() {
        let token = CancellationToken::new();
        assert_eq!(until_shutdown(&token, async { 7 }).await, Some(7));
    }

    #[tokio::test]
    async fn yields_none_once_the_token_is_cancelled() {
        let token = CancellationToken::new();
        token.cancel();
        // Biased select: an already-ready future must not win over a token
        // that has already fired.
        assert_eq!(until_shutdown(&token, async { 7 }).await, None);
    }

    #[tokio::test]
    async fn yields_none_when_the_token_fires_while_parked() {
        let token = CancellationToken::new();
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            cancel.cancel();
        });
        let parked = std::future::pending::<()>();
        assert_eq!(until_shutdown(&token, parked).await, None);
    }

    /// Serves `handler` on an ephemeral port with axum's graceful shutdown
    /// wired to `token`, sends one request to it, waits until the handler is
    /// actually parked inside the server, then cancels. Returns whether the
    /// serve future finished within `budget`.
    async fn serve_completes_after_cancel(
        park_forever: bool,
        budget: Duration,
    ) -> (bool, CancellationToken) {
        let token = CancellationToken::new();
        let in_handler = Arc::new(Notify::new());

        let handler_token = token.clone();
        let handler_entered = in_handler.clone();
        let app = Router::new().route(
            "/hang",
            get(move || async move {
                handler_entered.notify_one();
                if park_forever {
                    // The pre-fix websocket loops: park with no view of the
                    // shutdown token at all.
                    std::future::pending::<()>().await;
                } else {
                    until_shutdown(&handler_token, std::future::pending::<()>()).await;
                }
                "bye"
            }),
        );

        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");

        let serve_token = token.clone();
        let serve = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move { serve_token.cancelled().await })
                .await
        });

        let client = reqwest::Client::new();
        let request = tokio::spawn(async move {
            let _ = client
                .get(format!("http://{addr}/hang"))
                .timeout(Duration::from_secs(30))
                .send()
                .await;
        });

        // Only cancel once the request is genuinely in flight — otherwise the
        // server could shut down before it ever accepted the connection and
        // the test would pass for the wrong reason.
        tokio::time::timeout(Duration::from_secs(5), in_handler.notified())
            .await
            .expect("handler should have been entered");
        token.cancel();

        let finished = tokio::time::timeout(budget, serve).await.is_ok();
        request.abort();
        (finished, token)
    }

    /// Negative control: this is the shape both websocket handlers had. If
    /// this ever starts finishing, the test below has stopped proving
    /// anything.
    #[tokio::test]
    async fn a_handler_that_ignores_the_token_blocks_graceful_shutdown() {
        let (finished, _token) =
            serve_completes_after_cancel(true, Duration::from_millis(750)).await;
        assert!(
            !finished,
            "graceful shutdown should still be waiting on the parked handler"
        );
    }

    #[tokio::test]
    async fn a_handler_that_watches_the_token_lets_graceful_shutdown_finish() {
        let (finished, _token) = serve_completes_after_cancel(false, Duration::from_secs(5)).await;
        assert!(
            finished,
            "graceful shutdown should finish once the handler returns"
        );
    }
}
