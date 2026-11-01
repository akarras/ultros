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

    fn arena_diag() -> (u64, u64, u64, u64, u64) {
        leptos::reactive::owner::arena_diagnostics()
    }

    /// Counters for [`completed_render_drops_every_arena_value_it_created`].
    #[derive(Clone, Default)]
    struct DropCounters {
        built: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
        root_owner: Arc<std::sync::Mutex<Option<leptos::reactive::owner::Owner>>>,
    }

    struct Counted(Arc<AtomicUsize>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn counted(counters: &DropCounters) -> Counted {
        counters.built.fetch_add(1, Ordering::SeqCst);
        Counted(counters.dropped.clone())
    }

    /// A page whose components stash [`Counted`] values in the reactive
    /// arena through every storage shape the item page uses: signals, memos,
    /// stored values, a resource fetcher, `Suspense` children, and `For`
    /// rows (each row gets its own owner).
    #[leptos::component]
    fn ArenaProbePage() -> impl leptos::IntoView {
        use leptos::prelude::*;

        let counters = use_context::<DropCounters>().expect("counters context");
        let _sig = RwSignal::new(counted(&counters));
        let _stored = StoredValue::new(counted(&counters));
        let memo_token = counted(&counters);
        let memo = Memo::new(move |_| {
            let _ = &memo_token;
            3
        });
        let resource = Resource::new(|| (), {
            let counters = counters.clone();
            move |_| {
                let counters = counters.clone();
                async move {
                    let _probe = StoredValue::new(counted(&counters));
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    7
                }
            }
        });
        let row_counters = counters.clone();
        let child_counters = counters.clone();
        let title_counters = counters.clone();
        // Second-wave data: only created once `resource` resolves, so its
        // rows are built during the resolve walk rather than the first one.
        let rows_resource = Resource::new(move || resource.get(), {
            let counters = counters.clone();
            move |ready: Option<i32>| {
                let counters = counters.clone();
                async move {
                    let _probe = StoredValue::new(counted(&counters));
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    ready.map(|_| (0..6).collect::<Vec<i32>>())
                }
            }
        });
        let wave_counters = counters.clone();
        view! {
                    <leptos_meta::Title text=move || {
                        let _probe = StoredValue::new(counted(&title_counters));
                        "probe".to_string()
                    } />
                    <p>{move || memo.get()}</p>
                    <Suspense fallback=|| view! { <p>"loading"</p> }>
                        {move || {
                            let counters = child_counters.clone();
                            resource.get().map(move |value| {
                                let _inner = RwSignal::new(counted(&counters));
                                view! { <p>{value}</p> }
                            })
                        }}
                    </Suspense>
                    <Suspense fallback=|| view! { <p>"rows"</p> }>
                        {move || {
                            let counters = wave_counters.clone();
                            rows_resource.get().flatten().map(move |rows| {
                                let counters = counters.clone();
                                view! {
                                    <For
                                        each=move || rows.clone()
                                        key=|i| *i
                                        children={
                                            let counters = counters.clone();
                                            move |i| {
                                                let _wave_row =
                                                    RwSignal::new(counted(&counters));
                                                view! { <span>{i}</span> }
                                            }
                                        }
                                    />
                                }
                            })
                        }}
                    </Suspense>
                    <For
                        each=move || 0..6
                        key=|i| *i
                        children={
                            let counters = row_counters.clone();
                            move |i| {
                                let _row = RwSignal::new(counted(&counters));
                                view! { <span>{i}</span> }
                            }
                        }
                    />
        }
    }

    fn arena_probe_shell(_options: leptos::config::LeptosOptions) -> impl leptos::IntoView {
        use leptos::prelude::*;
        use leptos_meta::{MetaTags, provide_meta_context};
        use leptos_router::{
            components::{Route, Router, Routes},
            path,
        };

        provide_meta_context();
        let counters = use_context::<DropCounters>().expect("counters context");
        {
            let mut root = counters.root_owner.lock().unwrap();
            if root.is_none() {
                *root = Owner::current();
            }
        }
        view! {
            <html>
                <head>
                    <MetaTags />
                </head>
                <body>
                    <Router>
                        <Routes fallback=|| view! { <p>"nf"</p> }>
                            <Route path=path!("probe") view=ArenaProbePage />
                        </Routes>
                    </Router>
                </body>
            </html>
        }
    }

    /// Prod heap profiles (2026-11-01) showed arena items created during
    /// SSR item-page renders surviving for hours: +550 MB sampled through
    /// `ArenaItem::new_with_storage` in one hour at flat concurrency, RSS
    /// climbing ~0.5 GB/h until swap and the cgroup cap absorbed it. Every
    /// value a render stores in the arena must be dropped once the response
    /// stream has finished and leptos has run its forced owner cleanup —
    /// anything left over is retained in an arena no cleanup will visit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn completed_render_drops_every_arena_value_it_created() {
        let _ = any_spawner::Executor::init_tokio();

        for render in 0..3 {
            let counters = DropCounters::default();
            let options = leptos::config::LeptosOptions::builder()
                .output_name("ssr-arena-probe")
                .site_root("/nonexistent-site-root")
                .build();
            let fallback = leptos_axum::file_and_error_handler_with_context::<
                leptos::config::LeptosOptions,
                _,
            >(
                {
                    let counters = counters.clone();
                    move || {
                        leptos::prelude::provide_context(counters.clone());
                    }
                },
                arena_probe_shell,
            );
            let request = Request::builder()
                .uri("/probe")
                .body(Body::empty())
                .unwrap();
            let response =
                render_task(fallback(request.uri().clone(), State(options), request)).await;
            let body = response.into_body().collect().await.unwrap().to_bytes();
            assert!(
                String::from_utf8_lossy(&body).contains(">7<"),
                "render {render} never resolved its resource: {}",
                String::from_utf8_lossy(&body)
                    .chars()
                    .take(2000)
                    .collect::<String>()
            );

            // Give detached effect tasks and the drain path their chance to
            // finish; the forced cleanup at end-of-stream should already
            // have removed every node this render registered.
            let settled = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let built = counters.built.load(Ordering::SeqCst);
                    if built > 0 && built == counters.dropped.load(Ordering::SeqCst) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await;
            let built = counters.built.load(Ordering::SeqCst);
            let dropped = counters.dropped.load(Ordering::SeqCst);
            assert!(
                settled.is_ok(),
                "render {render} left arena values behind: built {built}, \
                 dropped {dropped}"
            );
            assert!(built >= 10, "probe page built only {built} values");
            let root = counters
                .root_owner
                .lock()
                .unwrap()
                .clone()
                .expect("shell captured a root owner");
            assert_eq!(
                root.arena_len(),
                0,
                "render {render} left values in its request arena after cleanup"
            );
        }
    }

    /// The same probe through the prod item-route shape instead of the
    /// fallback's render-to-string: in-order streaming bodies produced by
    /// detached render tasks, polled slowly so several renders interleave
    /// on the shared worker threads. Cross-request arena/owner mix-ups on
    /// those thread-locals only appear under this overlap.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_streamed_renders_drop_their_arena_values() {
        use axum::response::IntoResponse;

        let _ = any_spawner::Executor::init_tokio();
        let diag_before = arena_diag();

        let all_counters: Vec<DropCounters> = (0..8).map(|_| DropCounters::default()).collect();
        let mut joins = Vec::new();
        for counters in all_counters.clone() {
            joins.push(tokio::spawn(async move {
                let options = leptos::config::LeptosOptions::builder()
                    .output_name("ssr-arena-probe")
                    .site_root("/nonexistent-site-root")
                    .build();
                let handler = leptos_axum::render_app_to_stream_in_order_with_context(
                    {
                        let counters = counters.clone();
                        move || {
                            leptos::prelude::provide_context(counters.clone());
                        }
                    },
                    move || arena_probe_shell(options.clone()),
                );
                let request = Request::builder()
                    .uri("/probe")
                    .body(Body::empty())
                    .unwrap();
                let response =
                    detach_render(async move { handler(request).await.into_response() }).await;
                let mut body = response.into_body();
                let mut html = Vec::new();
                while let Some(frame) = body.frame().await {
                    if let Ok(bytes) = frame.expect("frame").into_data() {
                        html.extend_from_slice(&bytes);
                    }
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                assert!(
                    String::from_utf8_lossy(&html).contains(">7<"),
                    "streamed render never resolved its resource"
                );
            }));
        }
        for join in joins {
            join.await.expect("render task panicked");
        }

        for (render, counters) in all_counters.iter().enumerate() {
            let settled = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let built = counters.built.load(Ordering::SeqCst);
                    if built > 0 && built == counters.dropped.load(Ordering::SeqCst) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await;
            let built = counters.built.load(Ordering::SeqCst);
            let dropped = counters.dropped.load(Ordering::SeqCst);
            assert!(
                settled.is_ok(),
                "streamed render {render} left arena values behind: built \
                 {built}, dropped {dropped}"
            );
        }

        let diag_after = arena_diag();
        eprintln!(
            "arena diagnostics over streamed renders: inserts +{}, \
             unregistered +{}, cleanup removes +{}, cleanup misses +{}, \
             explicit removes +{}",
            diag_after.0 - diag_before.0,
            diag_after.1 - diag_before.1,
            diag_after.2 - diag_before.2,
            diag_after.3 - diag_before.3,
            diag_after.4 - diag_before.4,
        );
        for (render, counters) in all_counters.iter().enumerate() {
            let root = counters
                .root_owner
                .lock()
                .unwrap()
                .clone()
                .expect("shell captured a root owner");
            assert_eq!(
                root.arena_len(),
                0,
                "streamed render {render} left values in its request arena \
                 after cleanup"
            );
        }
    }

    /// Abandoned variant: the client takes the first chunk and leaves, so
    /// the render finishes on the `DrainOnDrop` background task. Bots do
    /// this to prod constantly; the drained render must still end with an
    /// empty request arena.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn abandoned_streamed_renders_drop_their_arena_values() {
        use axum::response::IntoResponse;

        let _ = any_spawner::Executor::init_tokio();

        for render in 0..3 {
            let counters = DropCounters::default();
            let options = leptos::config::LeptosOptions::builder()
                .output_name("ssr-arena-probe")
                .site_root("/nonexistent-site-root")
                .build();
            let handler = leptos_axum::render_app_to_stream_in_order_with_context(
                {
                    let counters = counters.clone();
                    move || {
                        leptos::prelude::provide_context(counters.clone());
                    }
                },
                move || arena_probe_shell(options.clone()),
            );
            let request = Request::builder()
                .uri("/probe")
                .body(Body::empty())
                .unwrap();
            let response =
                detach_render(async move { handler(request).await.into_response() }).await;
            let mut body = response.into_body();
            let first = body.frame().await;
            assert!(matches!(first, Some(Ok(_))), "no first chunk to abandon");
            drop(body);

            let settled = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let built = counters.built.load(Ordering::SeqCst);
                    if built > 0 && built == counters.dropped.load(Ordering::SeqCst) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await;
            let built = counters.built.load(Ordering::SeqCst);
            let dropped = counters.dropped.load(Ordering::SeqCst);
            assert!(
                settled.is_ok(),
                "abandoned render {render} left arena values behind: built \
                 {built}, dropped {dropped}"
            );
            let root = counters
                .root_owner
                .lock()
                .unwrap()
                .clone()
                .expect("shell captured a root owner");
            assert_eq!(
                root.arena_len(),
                0,
                "abandoned render {render} left values in its request arena"
            );
        }
    }

    /// The synthetic probe pages above clean up after themselves; prod's
    /// item page does not. Render the real app shell at an item route
    /// (stub API router: every fetch 404s, so resources settle as errors,
    /// but the game-data-driven tree — recipes, related items, charts —
    /// still builds) and require the request arena to empty each time.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn real_item_page_render_empties_its_arena() {
        use axum::response::IntoResponse;
        use std::sync::Mutex as StdMutex;
        use ultros_api_types::{
            world::{Datacenter, Region, World, WorldData},
            world_helper::WorldHelper,
        };
        use ultros_app::ssr_api::SsrApi;

        let _ = any_spawner::Executor::init_tokio();

        let worlds = Arc::new(WorldHelper::new(WorldData {
            regions: vec![Region {
                id: 1,
                name: "North-America".to_string(),
                datacenters: vec![Datacenter {
                    id: 1,
                    name: "Aether".to_string(),
                    region_id: 1,
                    worlds: vec![World {
                        id: 63,
                        name: "Gilgamesh".to_string(),
                        datacenter_id: 1,
                    }],
                }],
            }],
        }));
        let api = SsrApi::new(axum::Router::new());

        for render in 0..3 {
            let root_slot: Arc<StdMutex<Option<leptos::reactive::owner::Owner>>> =
                Arc::new(StdMutex::new(None));
            let options = leptos::config::LeptosOptions::builder()
                .output_name("ultros")
                .site_root("/nonexistent-site-root")
                .build();
            let handler = leptos_axum::render_app_to_stream_in_order_with_context(
                {
                    let worlds = worlds.clone();
                    let api = api.clone();
                    let root_slot = root_slot.clone();
                    move || {
                        *root_slot.lock().unwrap() = leptos::reactive::owner::Owner::current();
                        leptos::prelude::provide_context(ultros_app::LocalWorldData(Ok(
                            worlds.clone()
                        )));
                        leptos::prelude::provide_context(ultros_app::GuessedRegion(
                            "North-America".to_string(),
                        ));
                        leptos::prelude::provide_context(ultros_app::BootstrapUser(None));
                        leptos::prelude::provide_context(ultros_app::ListsReadonly(false));
                        leptos::prelude::provide_context(api.clone());
                    }
                },
                move || {
                    ultros_app::shell(
                        options.clone(),
                        "window.__ULTROS_BOOTSTRAP__=null;".to_string(),
                        false,
                    )
                },
            );
            let request = Request::builder()
                .uri("/item/Gilgamesh/44135")
                .body(Body::empty())
                .unwrap();
            let response =
                detach_render(async move { handler(request).await.into_response() }).await;
            let body = response.into_body().collect().await.unwrap().to_bytes();
            assert!(!body.is_empty(), "render {render} produced an empty body");
            // Let detached effect/resource tasks observe the teardown.
            tokio::time::sleep(Duration::from_millis(250)).await;
            let root = root_slot
                .lock()
                .unwrap()
                .clone()
                .expect("additional_context captured the root owner");
            assert_eq!(
                root.arena_len(),
                0,
                "item render {render} left values in its request arena"
            );
        }
    }

    /// The real item page under prod-like overlap: a slow stub API (every
    /// fetch takes 120 ms, like the slow-query warnings in prod logs),
    /// eight renders in flight at once, bodies polled slowly so renders
    /// interleave across the shared workers for their whole lifetimes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_real_item_page_renders_empty_their_arenas() {
        use axum::response::IntoResponse;
        use std::sync::Mutex as StdMutex;
        use ultros_api_types::{
            world::{Datacenter, Region, World, WorldData},
            world_helper::WorldHelper,
        };
        use ultros_app::ssr_api::SsrApi;

        let _ = any_spawner::Executor::init_tokio();
        let diag_before = arena_diag();

        let worlds = Arc::new(WorldHelper::new(WorldData {
            regions: vec![Region {
                id: 1,
                name: "North-America".to_string(),
                datacenters: vec![Datacenter {
                    id: 1,
                    name: "Aether".to_string(),
                    region_id: 1,
                    worlds: vec![World {
                        id: 63,
                        name: "Gilgamesh".to_string(),
                        datacenter_id: 1,
                    }],
                }],
            }],
        }));
        // Serve a captured real listings payload (path via env var) so the
        // listings subtree builds for real; `{}` for everything else.
        let listings_payload: Arc<str> = std::env::var("ULTROS_TEST_LISTINGS_JSON")
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(Arc::from)
            .unwrap_or_else(|| Arc::from("{}"));
        let api = SsrApi::new(axum::Router::new().fallback({
            let listings_payload = listings_payload.clone();
            move |req: axum::extract::Request| {
                let listings_payload = listings_payload.clone();
                async move {
                    tokio::time::sleep(Duration::from_millis(120)).await;
                    if req.uri().path().starts_with("/api/v1/listings/") {
                        (axum::http::StatusCode::OK, listings_payload.to_string())
                    } else {
                        (axum::http::StatusCode::OK, "{}".to_string())
                    }
                }
            }
        }));

        let root_slots: Vec<Arc<StdMutex<Option<leptos::reactive::owner::Owner>>>> =
            (0..8).map(|_| Arc::new(StdMutex::new(None))).collect();
        let mut joins = Vec::new();
        for (index, root_slot) in root_slots.clone().into_iter().enumerate() {
            let worlds = worlds.clone();
            let api = api.clone();
            joins.push(tokio::spawn(async move {
                let options = leptos::config::LeptosOptions::builder()
                    .output_name("ultros")
                    .site_root("/nonexistent-site-root")
                    .build();
                let add_context = {
                    let root_slot = root_slot.clone();
                    move || {
                        *root_slot.lock().unwrap() = leptos::reactive::owner::Owner::current();
                        leptos::prelude::provide_context(ultros_app::LocalWorldData(Ok(
                            worlds.clone()
                        )));
                        leptos::prelude::provide_context(ultros_app::GuessedRegion(
                            "North-America".to_string(),
                        ));
                        leptos::prelude::provide_context(ultros_app::BootstrapUser(None));
                        leptos::prelude::provide_context(ultros_app::ListsReadonly(false));
                        leptos::prelude::provide_context(api.clone());
                    }
                };
                let app_fn = move || {
                    ultros_app::shell(
                        options.clone(),
                        "window.__ULTROS_BOOTSTRAP__=null;".to_string(),
                        false,
                    )
                };
                let request = Request::builder()
                    .uri("/item/Gilgamesh/44135")
                    .body(Body::empty())
                    .unwrap();
                // Alternate prod's two streaming modes so in-order and
                // out-of-order renders interleave on the shared workers.
                let response = detach_render(async move {
                    if index % 2 == 0 {
                        let handler = leptos_axum::render_app_to_stream_in_order_with_context(
                            add_context,
                            app_fn,
                        );
                        handler(request).await.into_response()
                    } else {
                        let handler =
                            leptos_axum::render_app_to_stream_with_context_and_replace_blocks(
                                add_context,
                                app_fn,
                                true,
                            );
                        handler(request).await.into_response()
                    }
                })
                .await;
                let mut body = response.into_body();
                while let Some(frame) = body.frame().await {
                    frame.expect("frame");
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            }));
        }
        for join in joins {
            join.await.expect("render task panicked");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;

        let diag_after = arena_diag();
        eprintln!(
            "arena diagnostics over concurrent item renders: inserts +{}, \
             unregistered +{}, cleanup removes +{}, cleanup misses +{}, \
             explicit removes +{}",
            diag_after.0 - diag_before.0,
            diag_after.1 - diag_before.1,
            diag_after.2 - diag_before.2,
            diag_after.3 - diag_before.3,
            diag_after.4 - diag_before.4,
        );
        for (render, root_slot) in root_slots.iter().enumerate() {
            let root = root_slot
                .lock()
                .unwrap()
                .clone()
                .expect("additional_context captured the root owner");
            assert_eq!(
                root.arena_len(),
                0,
                "concurrent item render {render} left values in its arena"
            );
        }
    }
}
