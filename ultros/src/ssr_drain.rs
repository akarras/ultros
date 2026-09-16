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
    extract::State,
    http::{Request, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::BodyExt;
use leptos::reactive::owner::Sandboxed;
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
        render_task(render)
            .bind_hub(sentry::Hub::current())
            .in_current_span(),
    );
    match task.await {
        Ok(response) => response,
        Err(join) => {
            // `warn!`, not `error!`: the panic hook has already reported the
            // panic itself (with its location), and an `error!` here became a
            // second GlitchTip issue per URL ("SSR render task failed",
            // #7339–#7386) that only ever restated the panic message.
            tracing::warn!(error = %join, "SSR render task failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// The future [`detach_render`] runs on its task: the render, with its body
/// wrapped in [`DrainOnDrop`], under a [`StickyArena`].
fn render_task<F>(render: F) -> impl Future<Output = Response> + Send + 'static
where
    F: Future<Output = Response> + Send + 'static,
{
    StickyArena::new(async move { render.await.map(DrainOnDrop::wrap) })
}

/// Keeps a render's reactive arena active across its await points.
///
/// leptos' arena (where every signal lives) is a thread-local that a
/// `Sandboxed` poll sets and nothing ever restores, so it only follows the
/// request while one of the request's `Sandboxed` futures is being polled.
/// `leptos_integration_utils::from_app` reads reactive values *between*
/// those polls: `inject_meta_context` evaluates every `<Title>` closure
/// after a `tick().await`. Whatever the worker thread ran in the meantime
/// decides which arena that read sees — another request's ("you tried to
/// access a reactive value … already disposed") or none ("the
/// `sandboxed-arenas` feature is active, but no Arena is active") — and the
/// render panics. GlitchTip #7382 / #7383 (the 404 page's title) and the
/// item page's title (`xiv_data.rs` reading the locale).
///
/// This wrapper re-activates, before every poll, the arena that was active
/// when the previous poll returned: that is the request's own arena, set by
/// the last `Sandboxed` poll inside the render. `Sandboxed` is the only
/// public handle on the thread-local — constructing one captures the current
/// arena, and polling it re-activates that arena before polling its inner
/// future, which here is never ready.
struct StickyArena<F> {
    inner: Pin<Box<F>>,
    arena: Option<Sandboxed<std::future::Pending<()>>>,
}

impl<F> StickyArena<F> {
    fn new(inner: F) -> Self {
        Self {
            inner: Box::pin(inner),
            arena: None,
        }
    }
}

impl<F: Future> Future for StickyArena<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(arena) = this.arena.as_mut() {
            // Only ever `Pending`; polled for its side effect of setting the
            // arena. `Pending` registers no waker, so nothing leaks.
            let _ = Pin::new(arena).poll(cx);
        }
        let polled = this.inner.as_mut().poll(cx);
        if polled.is_pending() {
            this.arena = Some(Sandboxed::new(std::future::pending()));
        }
        polled
    }
}

/// The shape of `leptos_axum::file_and_error_handler_with_context`'s handler.
pub type FallbackFuture = Pin<Box<dyn Future<Output = Response> + Send + 'static>>;

/// Wraps the file/404 fallback handler so its render runs through
/// [`detach_render`] like every leptos route handler does.
///
/// The fallback renders the whole app in-order (collecting it to a `String`
/// before the first byte goes out), so a scanner that gives up on an unknown
/// path cancels the handler future mid-render and tears the owner down under
/// the Suspense tasks — the not-found variant of #7269 (GlitchTip #7382,
/// #7383).
pub fn detach_fallback<S, H>(
    inner: H,
) -> impl Fn(Uri, State<S>, Request<Body>) -> FallbackFuture + Clone + Send + Sync + 'static
where
    S: Clone + Send + Sync + 'static,
    H: Fn(Uri, State<S>, Request<Body>) -> FallbackFuture + Clone + Send + Sync + 'static,
{
    move |uri, state, req| Box::pin(detach_render(inner(uri, state, req)))
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

    use axum::{
        Router,
        body::Body,
        extract::State,
        http::{Request, StatusCode, Uri},
        response::IntoResponse,
    };
    use futures::{StreamExt, stream};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::{DrainOnDrop, FallbackFuture, detach_fallback, detach_render, render_task};

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

    /// A stand-in for leptos' file/404 fallback: renders for 30ms, then
    /// answers 404 with a body whose end-of-stream hook bumps `completed`.
    fn slow_fallback(
        rendered: Arc<AtomicUsize>,
        completed: Arc<AtomicUsize>,
    ) -> impl Fn(Uri, State<()>, Request<Body>) -> FallbackFuture + Clone + Send + Sync + 'static
    {
        move |_uri, State(()), _req| {
            let rendered = rendered.clone();
            let completed = completed.clone();
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                rendered.fetch_add(1, Ordering::SeqCst);
                (StatusCode::NOT_FOUND, tracked_body(3, completed)).into_response()
            })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_fallback_still_finishes_rendering() {
        let completed = Arc::new(AtomicUsize::new(0));
        let rendered = Arc::new(AtomicUsize::new(0));
        let fallback = detach_fallback(slow_fallback(rendered.clone(), completed.clone()));
        let mut handler = fallback(
            Uri::from_static("/missing"),
            State(()),
            Request::new(Body::empty()),
        );
        let pending = poll_fn(|cx| Poll::Ready(handler.as_mut().poll(cx).is_pending())).await;
        assert!(pending, "render should not have finished on the first poll");
        drop(handler);
        assert!(
            wait_for(&rendered, 1).await,
            "fallback render was cancelled"
        );
        assert!(
            wait_for(&completed, 1).await,
            "the abandoned fallback body was dropped instead of drained"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn detached_fallback_is_an_axum_handler() {
        let completed = Arc::new(AtomicUsize::new(0));
        let rendered = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .fallback(detach_fallback(slow_fallback(rendered, completed)))
            .with_state(());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/missing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"chunkchunkchunk");
    }

    /// The shell for [`title_is_read_under_the_request_arena`]: a real leptos
    /// page whose `<Title>` text is a closure over a signal, like every
    /// `MetaTitle` in the app.
    fn titled_shell(_options: leptos::config::LeptosOptions) -> impl leptos::IntoView {
        use leptos::prelude::*;
        use leptos_meta::{MetaTags, Title, provide_meta_context};

        provide_meta_context();
        let title = RwSignal::new(String::from("Into the void"));
        view! {
            <html>
                <head>
                    <MetaTags />
                </head>
                <body>
                    <Title text=move || title.get() />
                    <main>"404"</main>
                </body>
            </html>
        }
    }

    /// GlitchTip #7382 / #7383: leptos evaluates `<Title>` closures in
    /// `inject_meta_context`, after an await and outside any `Sandboxed`
    /// scope. The reactive arena is a thread-local that a `Sandboxed` poll
    /// sets and nothing restores, so that read sees whatever the worker
    /// thread last ran — another request's arena ("you tried to access a
    /// reactive value … already disposed") or none ("no Arena is active") —
    /// and the render panics.
    ///
    /// The render task is driven by hand here so the test can do what a busy
    /// worker does between two polls of it: run a different request, which
    /// leaves that request's arena active on the thread.
    #[tokio::test]
    async fn title_is_read_under_the_request_arena() {
        use leptos::prelude::Owner;

        // Route handlers do this on their first request; the fallback alone
        // does not, and leptos_meta / Suspense spawn reactive tasks.
        let _ = any_spawner::Executor::init_tokio();

        // The other request. Built off-thread so creating it doesn't touch
        // this thread's reactive thread-locals; `with` is what a `Sandboxed`
        // poll of that request does to the arena (the owner is restored).
        let other = std::thread::spawn(|| Owner::new_root(None)).join().unwrap();

        let options = leptos::config::LeptosOptions::builder()
            .output_name("ssr-drain-test")
            .site_root("/nonexistent-site-root")
            .build();
        let fallback = leptos_axum::file_and_error_handler_with_context::<
            leptos::config::LeptosOptions,
            _,
        >(|| {}, titled_shell);
        let request = Request::builder()
            .uri("/missing")
            .body(Body::empty())
            .unwrap();
        let mut task = Box::pin(render_task(fallback(
            request.uri().clone(),
            State(options),
            request,
        )));

        let mut polls = 0usize;
        let response = poll_fn(|cx| {
            polls += 1;
            let polled = task.as_mut().poll(cx);
            if polled.is_pending() {
                other.with(|| {});
            }
            polled
        })
        .await;
        assert!(polls > 1, "the render never yielded, so nothing was tested");

        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "the 404 render panicked reading its <Title> under a foreign arena"
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = String::from_utf8_lossy(&body);
        assert!(
            html.contains("<title>Into the void</title>"),
            "title missing from the rendered page: {html}"
        );
    }
}
