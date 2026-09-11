//! Keeps an abandoned SSR render alive until leptos is finished with it.
//!
//! leptos ties the request's reactive `Owner` to the response body: the body
//! stream carries the owner and cleans it up after the last chunk
//! (`leptos_integration_utils::build_response`). When the client goes away
//! mid-render, hyper drops the body (or, before the first chunk is out, the
//! whole handler future) on one of its worker threads, and that drop tears the
//! owner down — arena nodes removed, child owners cleaned, context chain
//! severed — while render work is still running elsewhere. Every `<Suspense>`
//! boundary spawns a detached `Effect::new_isomorphic` task whose first run is
//! unconditional and which walks the children with `dry_resolve()`, building
//! components as it goes. That walk then reads a signal or context that was
//! just disposed underneath it, and the server panics with "Tried to access a
//! reactive value that has already been disposed" / "I18n context is missing"
//! / `Option::unwrap()` on a `None` value — GlitchTip #7269 and its siblings
//! (#7319, #7315, #7314, #7316).
//!
//! The fix is to never drop a live render: a body dropped before end-of-stream
//! is handed to a background task that polls it to completion, so the owner is
//! cleaned up by leptos' own end-of-stream hook, after every boundary has
//! resolved and disposed its effect. The handler future itself is run on a
//! detached task for the same reason. The drain is capped so an abandoned
//! render cannot pin server resources forever.

use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    body::{Body, Bytes},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::BodyExt;
use sentry::SentryFutureExt;
use tracing::Instrument;

/// Longest an abandoned render is kept alive after the client disconnects.
/// Resources for the page are already in flight regardless, so the only cost
/// past this point is the HTML walk itself; anything slower than this is a
/// stuck request and the pre-fix behaviour (drop it) is the right fallback.
pub const DRAIN_CAP: Duration = Duration::from_secs(60);

/// A response body that finishes draining its inner body in the background if
/// it is dropped before end-of-stream.
pub struct DrainOnDrop {
    inner: Option<Body>,
    finished: bool,
}

impl DrainOnDrop {
    /// Wraps `body` so an early drop drains it instead of cancelling it.
    pub fn wrap(body: Body) -> Body {
        Body::new(Self {
            inner: Some(body),
            finished: false,
        })
    }
}

impl HttpBody for DrainOnDrop {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let Some(inner) = this.inner.as_mut() else {
            return Poll::Ready(None);
        };
        let polled = Pin::new(inner).poll_frame(cx);
        if matches!(polled, Poll::Ready(None) | Poll::Ready(Some(Err(_)))) {
            this.finished = true;
        }
        polled
    }

    fn is_end_stream(&self) -> bool {
        self.finished || self.inner.as_ref().is_none_or(|b| b.is_end_stream())
    }

    fn size_hint(&self) -> SizeHint {
        self.inner
            .as_ref()
            .map(HttpBody::size_hint)
            .unwrap_or_default()
    }
}

impl Drop for DrainOnDrop {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let Some(mut inner) = self.inner.take() else {
            return;
        };
        if inner.is_end_stream() {
            return;
        }
        // Dropped outside a runtime (shutdown, tests without one): nothing can
        // poll it, so fall back to the plain drop.
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(
            async move {
                let drained = tokio::time::timeout(DRAIN_CAP, async {
                    while let Some(frame) = inner.frame().await {
                        if frame.is_err() {
                            break;
                        }
                    }
                })
                .await;
                if drained.is_err() {
                    tracing::warn!(
                        cap_secs = DRAIN_CAP.as_secs(),
                        "abandoned SSR response still rendering past the drain cap; dropping it"
                    );
                }
            }
            .bind_hub(sentry::Hub::current())
            .in_current_span(),
        );
    }
}

/// Runs an SSR handler on a detached task so a client disconnect cannot cancel
/// the render midway, and wraps the produced body in [`DrainOnDrop`].
///
/// The caller's sentry hub and tracing span follow the work onto the task.
/// A render that panics becomes a 500 here (tokio catches the unwind); the
/// panic itself is still reported by the sentry panic hook.
pub async fn detach_render<F>(render: F) -> Response
where
    F: Future<Output = Response> + Send + 'static,
{
    let task = tokio::spawn(
        async move { render.await.map(DrainOnDrop::wrap) }
            .bind_hub(sentry::Hub::current())
            .in_current_span(),
    );
    match task.await {
        Ok(response) => response,
        Err(join) => {
            tracing::error!(error = %join, "SSR render task failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::poll_fn,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        task::Poll,
        time::Duration,
    };

    use axum::body::Body;
    use futures::{StreamExt, stream};
    use http_body_util::BodyExt;

    use super::{DrainOnDrop, detach_render};

    /// A body of `chunks` that bumps `completed` once its last chunk has been
    /// pulled — the stand-in for leptos' end-of-stream owner cleanup.
    fn tracked_body(chunks: usize, completed: Arc<AtomicUsize>) -> Body {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        // Keep the stream pending between chunks so a consumer cannot race
        // through it; each `tx.send` releases one chunk.
        let gate = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        // Detached on purpose: the feeder ends on its own once every chunk is
        // released or the body is gone.
        drop(tokio::spawn(async move {
            for _ in 0..chunks {
                tokio::time::sleep(Duration::from_millis(5)).await;
                if tx.send(()).is_err() {
                    break;
                }
            }
        }));
        let data = gate
            .take(chunks)
            .map(|()| Ok::<_, axum::Error>(axum::body::Bytes::from_static(b"chunk")));
        let done = stream::once(async move {
            completed.fetch_add(1, Ordering::SeqCst);
            Ok(axum::body::Bytes::new())
        });
        Body::from_stream(data.chain(done))
    }

    async fn wait_for(counter: &AtomicUsize, expected: usize) -> bool {
        tokio::time::timeout(Duration::from_secs(5), async {
            while counter.load(Ordering::SeqCst) < expected {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn body_dropped_early_is_drained_to_the_end() {
        let completed = Arc::new(AtomicUsize::new(0));
        let mut body = DrainOnDrop::wrap(tracked_body(4, completed.clone()));
        let first = body.frame().await.expect("first chunk").expect("ok");
        assert!(first.is_data());
        drop(body);
        assert!(
            wait_for(&completed, 1).await,
            "the end-of-stream hook never ran after the early drop"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fully_consumed_body_does_not_drain_twice() {
        let completed = Arc::new(AtomicUsize::new(0));
        let mut body = DrainOnDrop::wrap(tracked_body(2, completed.clone()));
        while let Some(frame) = body.frame().await {
            frame.expect("ok");
        }
        assert_eq!(completed.load(Ordering::SeqCst), 1);
        drop(body);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(completed.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_handler_still_finishes_rendering() {
        let completed = Arc::new(AtomicUsize::new(0));
        let rendered = Arc::new(AtomicUsize::new(0));
        let render = {
            let completed = completed.clone();
            let rendered = rendered.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                rendered.fetch_add(1, Ordering::SeqCst);
                axum::response::Response::new(tracked_body(3, completed))
            }
        };
        let mut handler = Box::pin(detach_render(render));
        // Poll once so the task is spawned, then abandon the handler future the
        // way hyper does when the connection closes.
        let pending = poll_fn(|cx| Poll::Ready(handler.as_mut().poll(cx).is_pending())).await;
        assert!(pending, "render should not have finished on the first poll");
        drop(handler);
        assert!(wait_for(&rendered, 1).await, "render was cancelled");
        assert!(
            wait_for(&completed, 1).await,
            "the abandoned response body was dropped instead of drained"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panicking_render_becomes_a_500() {
        let response = detach_render(async { panic!("boom") }).await;
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
