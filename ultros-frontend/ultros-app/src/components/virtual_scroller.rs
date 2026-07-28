use leptos::prelude::*;
use std::hash::Hash;
use std::{cell::RefCell, rc::Rc};
use web_sys::wasm_bindgen::JsCast;
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::{HtmlDivElement, ResizeObserver, window};

struct Fenwick {
    n: usize,
    bit: Vec<f64>,
}
impl Fenwick {
    fn new(n: usize) -> Self {
        Self {
            n,
            bit: vec![0.0; n + 1],
        }
    }
    fn reset(&mut self, n: usize) {
        self.n = n;
        self.bit.clear();
        self.bit.resize(n + 1, 0.0);
    }
    fn add(&mut self, mut idx: usize, delta: f64) {
        // fenwick tree is 1-based internally
        idx += 1;
        while idx <= self.n {
            self.bit[idx] += delta;
            idx += idx & (!idx + 1);
        }
    }
    fn sum(&self, mut idx: usize) -> f64 {
        // prefix sum of [0..idx)
        if self.n == 0 {
            return 0.0;
        }
        if idx > self.n {
            idx = self.n;
        }
        let mut res = 0.0;
        while idx > 0 {
            res += self.bit[idx];
            idx &= idx - 1;
        }
        res
    }
}

/// Where a [`VirtualScroller`] reads its scroll position and viewport size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollSource {
    /// The component owns a fixed-height `overflow-y-auto` container.
    /// This is the historical behavior and remains the default.
    Container { viewport_height: f64 },
    /// The page scrolls; the list measures against the window. Keeps
    /// native scrolling on mobile (no nested scroll trap, browser chrome
    /// auto-hides). `sticky_offset` is the height of sticky chrome above
    /// the list, so rows hidden behind it are not counted as visible.
    ///
    /// Note: in this mode the scroller element deliberately carries **no**
    /// `overflow` of its own. Any overflow on an ancestor of a
    /// `position: sticky` header makes that header stick to the ancestor's
    /// scrollport instead of the viewport, which would silently defeat the
    /// sticky header. A caller that needs horizontal scrolling must put
    /// `overflow-x-auto` *inside* the header/row views, not around the list.
    Window { sticky_offset: f64 },
}

/// Rows rendered during SSR and on the first client render in
/// [`ScrollSource::Window`] mode.
///
/// Window mode cannot measure `innerHeight` on the server. Rendering a
/// measured count on the client while the server rendered a different one
/// is an SSR/CSR mismatch, which surfaces as the tachys `hydration.rs`
/// panic this repo has hit repeatedly. Both sides therefore render exactly
/// this many rows until an `Effect` flips the `hydrated` flag.
pub const SSR_FALLBACK_ROWS: usize = 20;

/// Usable viewport height for a scroll source. Extracted so the geometry
/// is testable without a browser.
fn effective_viewport_for(source: ScrollSource, measured_window_height: f64) -> f64 {
    match source {
        ScrollSource::Container { viewport_height } => viewport_height,
        ScrollSource::Window { sticky_offset } => (measured_window_height - sticky_offset).max(0.0),
    }
}

/// Viewport height, in px, that the row math should actually use.
///
/// `measured_window_height` is `0.0` until the client has measured the window,
/// which is permanently the case on the server. In [`ScrollSource::Window`]
/// mode a measurement is only trusted once `hydrated` is true *and* the
/// measurement is non-zero; until then both sides return the same
/// [`SSR_FALLBACK_ROWS`]-worth of height, so the server render and the first
/// client render produce an identical row count.
///
/// [`ScrollSource::Container`] ignores both extra arguments entirely, which is
/// what keeps the existing call sites byte-identical.
fn viewport_px(
    source: ScrollSource,
    measured_window_height: f64,
    hydrated: bool,
    row_height: f64,
    header_height: f64,
) -> f64 {
    if matches!(source, ScrollSource::Window { .. }) && (!hydrated || measured_window_height <= 0.0)
    {
        return SSR_FALLBACK_ROWS as f64 * row_height;
    }
    (effective_viewport_for(source, measured_window_height) - header_height).max(0.0)
}

/// Rows to render for a viewport of `viewport` px, including `overscan` rows
/// beyond the fold. Extracted so the SSR fallback height can be checked to
/// round-trip back to exactly [`SSR_FALLBACK_ROWS`] rows.
fn rows_for_viewport(viewport: f64, avg_row_height: f64, overscan: u32) -> u32 {
    ((viewport / avg_row_height).ceil() as u32).max(1) + overscan
}

/// Virtual scroller currently mimics the API of the ForEach components, but adds a row_height and viewport_height.
/// It might be possible to not have a fixed row height in the future, but for now it's good enough!
///
/// Optional sticky header:
/// You can provide a header and header_height; the header will render sticky inside the scroll container,
/// and virtualization will account for the header height.
#[component]
pub fn VirtualScroller<T, D, V, KF, K>(
    each: Signal<Vec<T>>,
    key: KF,
    view: D,
    viewport_height: f64,
    /// Opt into window-scroll virtualization. `None` preserves the
    /// historical container behavior driven by `viewport_height`.
    #[prop(optional)]
    scroll_source: Option<ScrollSource>,
    row_height: f64,
    #[prop(optional, into)] header: Option<AnyView>,
    #[prop(optional)] header_height: f64,
    #[prop(optional)] overscan: u32,
    #[prop(optional)] variable_height: bool,
    #[prop(optional, into)] scroll_to_index: Option<Signal<Option<usize>>>,
    #[prop(optional)] scroller_ref: Option<NodeRef<leptos::html::Div>>,
    /// Handle on the element that holds the rows.
    ///
    /// That element already computes to `overflow-x: auto` (it declares
    /// `overflow-y: hidden`, which forces the visible axis to `auto`), so it
    /// is the list's horizontal scrollport. A caller rendering a grid wider
    /// than the viewport needs this handle to keep its own header scrollport
    /// in sync with it — the list itself cannot be wrapped in a scrollport
    /// without stealing the sticky header's (see [`ScrollSource::Window`]).
    #[prop(optional)]
    list_ref: Option<NodeRef<leptos::html::Div>>,
    /// CSS `min-width` for the column of rows, for a caller whose grid is
    /// wider than the viewport.
    ///
    /// Widening the rows alone is not enough to make the list scroll: the row
    /// box carries `contain: layout`, which stops its overflow reaching the
    /// list's scrollable overflow region (measured in Chrome — the rows
    /// overflow, `scrollWidth` on the list does not move). Sizing the spacer
    /// that holds the rows is what actually gives the list something to
    /// scroll. Takes any CSS length, so a `var()` can keep a responsive value
    /// in the stylesheet where it belongs.
    #[prop(optional, into)]
    row_min_width: Option<String>,
    /// Optional writeback of the rendered row range `(start, end)` (end
    /// exclusive, includes overscan). Lets a parent fetch data only for
    /// rows in view. When omitted, no extra work is done.
    #[prop(optional, into)]
    visible_range: Option<RwSignal<(usize, usize)>>,
) -> impl IntoView
where
    D: Fn(T) -> V + 'static + Clone + Send,
    V: IntoView + 'static,
    KF: Fn(&T) -> K + 'static + Clone + Send,
    K: Eq + Hash + 'static,
    T: 'static + Clone + Send + Sync + PartialEq,
{
    let render_ahead: u32 = if overscan == 0 { 10 } else { overscan };
    let header_h: f64 = header_height.max(0.0);
    let header_opt: Option<AnyView> = header;
    let source = scroll_source.unwrap_or(ScrollSource::Container { viewport_height });
    let is_window = matches!(source, ScrollSource::Window { .. });

    // Hydration gate. Effects run client-only and after hydration, so the
    // first client render still sees `false` and matches the server's.
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| hydrated.set(true));

    // Measured `window.innerHeight`, only meaningful once hydrated. Starts at
    // 0.0 on both sides so nothing can diverge before the first effect runs.
    let window_height = RwSignal::new(0.0f64);
    let (scroll_offset, set_scroll_offset) = signal(0);
    // rAF-based scroll coalescing to reduce state churn under heavy scroll
    let last_scroll = RwSignal::new(0);
    let raf_pending = RwSignal::new(false);
    // hybrid variable-height state: per-index delta from estimated row_height and prefix sums
    // ⚡ Bolt Optimization: Replace Memo::new with Signal::derive for O(1) ops
    let children_len = Signal::derive(move || each.with(|children| children.len()));
    let height_deltas = StoredValue::new(Vec::<f64>::new());
    let initial_len = each.with_untracked(|children| children.len());
    let fenwick = RwSignal::new(Fenwick::new(initial_len));

    // keep vectors sized to item count and reinitialize Fenwick when the dataset changes
    Effect::new(move |_| {
        let len = children_len();
        // reset measurements on length change
        let v = vec![0.0; len];
        height_deltas.set_value(v);
        fenwick.update(|f| {
            f.reset(len);
        });
        // reset scroll so new dataset renders from top (e.g., search changes).
        // In window mode the page scroll position is the source of truth and
        // did not move, so zeroing here would render the wrong slice until the
        // next scroll event.
        if !is_window {
            set_scroll_offset(0);
        }
    });

    // dataset reset handled by length change effect
    let scroller: NodeRef<leptos::html::Div> = match scroller_ref {
        Some(r) => r,
        None => NodeRef::<leptos::html::Div>::new(),
    };
    let list: NodeRef<leptos::html::Div> = match list_ref {
        Some(r) => r,
        None => NodeRef::<leptos::html::Div>::new(),
    };

    // Window-scroll mode: the container no longer scrolls, so its `on:scroll`
    // handler is inert. Drive `scroll_offset` and `window_height` from the
    // page instead.
    if is_window {
        let sticky_offset = match source {
            ScrollSource::Window { sticky_offset } => sticky_offset,
            ScrollSource::Container { .. } => 0.0,
        };
        // The JS closures are parked in a local StoredValue rather than
        // `Closure::forget`-ed: a forgotten listener keeps firing after the
        // component is disposed and writes into dead signals on the next
        // route change.
        let window_cb = StoredValue::new_local(None::<Closure<dyn FnMut()>>);
        // Handle of a frame that has been requested but has not fired yet.
        let raf_handle = StoredValue::new(None::<i32>);
        on_cleanup(move || {
            // Cancel an in-flight frame *before* dropping the closures. A
            // scroll immediately followed by a route change leaves a frame
            // already scheduled against the rAF closure; dropping it first
            // would have the browser invoke a freed wasm-bindgen closure
            // ("closure invoked recursively or after being dropped"), and even
            // surviving that it would run `sync()` into disposed signals.
            if let Some(handle) = raf_handle.get_value()
                && let Some(w) = window()
            {
                let _ = w.cancel_animation_frame(handle);
            }
            raf_handle.set_value(None);
            window_cb.update_value(|slot| {
                if let Some(cb) = slot.take()
                    && let Some(w) = window()
                {
                    let handler = cb.as_ref().unchecked_ref();
                    let _ = w.remove_event_listener_with_callback("scroll", handler);
                    let _ = w.remove_event_listener_with_callback("resize", handler);
                }
            });
        });

        Effect::new(move |_| {
            // Tracks nothing, so it runs exactly once; the guard is belt and
            // braces against a double registration.
            if window_cb.with_value(|slot| slot.is_some()) {
                return;
            }
            let Some(w) = window() else { return };

            // Reads layout, so it is only ever called from a rAF callback (or
            // once at setup) rather than synchronously on every scroll event.
            let sync = move || {
                if let Some(h) = window()
                    .and_then(|w| w.inner_height().ok())
                    .and_then(|v| v.as_f64())
                {
                    window_height.set(h);
                }
                if let Some(el) = scroller.get_untracked() {
                    // `rect.top()` is viewport-relative, so how far the list
                    // has scrolled is just how far its top edge has travelled
                    // above the viewport (plus any sticky chrome covering it).
                    // Measuring every frame rather than caching a `list_top`
                    // keeps this correct across reflows above the list, and
                    // avoids depending on layout having settled at setup time.
                    let top = el.get_bounding_client_rect().top();
                    set_scroll_offset((sticky_offset - top).max(0.0).round() as i32);
                }
            };
            sync();

            // One long-lived rAF closure, reused for every frame, so scrolling
            // does not leak a `Closure` per event.
            type RafCallback = Closure<dyn FnMut(f64)>;
            let raf_cb: Rc<RefCell<Option<RafCallback>>> = Rc::new(RefCell::new(None));
            *raf_cb.borrow_mut() = Some(Closure::wrap(Box::new(move |_: f64| {
                // The frame has fired, so the handle is spent; clearing it
                // keeps `on_cleanup` from cancelling a stale id.
                raf_handle.set_value(None);
                sync();
                raf_pending.set(false);
            }) as Box<dyn FnMut(f64)>));

            let cb = Closure::wrap(Box::new(move || {
                if raf_pending.get_untracked() {
                    return;
                }
                raf_pending.set(true);
                // Every path that fails to actually schedule a frame has to
                // clear the flag again, otherwise nothing will ever set it back
                // to false and this listener is dead for the rest of the
                // component's life.
                let scheduled = match (window(), raf_cb.borrow().as_ref()) {
                    (Some(w), Some(c)) => {
                        w.request_animation_frame(c.as_ref().unchecked_ref()).ok()
                    }
                    _ => None,
                };
                match scheduled {
                    Some(handle) => raf_handle.set_value(Some(handle)),
                    None => raf_pending.set(false),
                }
            }) as Box<dyn FnMut()>);

            let handler = cb.as_ref().unchecked_ref();
            let _ = w.add_event_listener_with_callback("scroll", handler);
            let _ = w.add_event_listener_with_callback("resize", handler);
            window_cb.set_value(Some(cb));
        });
    }

    // use memo here so our signals only retrigger if the value actually changed.
    let child_start = Memo::new(move |_| {
        let len = children_len();
        each.with(|_| ());
        if len == 0 {
            return 0u32;
        }
        // binary search for smallest i where i*row_height + prefix_sums[i] >= effective_scroll
        let effective_scroll = (scroll_offset() as f64 - header_h).max(0.0);

        let lo_u32 = fenwick.with(|f| {
            let mut lo: i32 = 0;
            let mut hi: i32 = len as i32;
            while lo < hi {
                let mid = (lo + hi) / 2;
                let base = mid as f64 * row_height;
                let delta = f.sum(mid as usize);
                if base + delta < effective_scroll {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            lo.max(0) as u32
        });

        lo_u32.saturating_sub(render_ahead / 2)
    });
    // In container mode this tracks `window_height` and `hydrated` needlessly,
    // but neither ever changes the result there, so the memo's own diffing
    // absorbs it and downstream signals never see a change.
    let effective_viewport = Memo::new(move |_| {
        viewport_px(
            source,
            window_height.get(),
            hydrated.get(),
            row_height,
            header_h,
        )
    });
    let avg_row_height = Memo::new(move |_| {
        let len = children_len();
        if len == 0 {
            row_height
        } else {
            let total_delta = fenwick.with(|f| f.sum(len));
            row_height + total_delta / len as f64
        }
    });
    let children_shown = Memo::new(move |_| {
        rows_for_viewport(effective_viewport.get(), avg_row_height(), render_ahead)
    });

    // Publish the rendered row range to an optional parent signal. `child_start`
    // and `children_shown` already account for overscan and match the slice used
    // by `virtual_children` below.
    if let Some(range_sig) = visible_range {
        Effect::new(move |_| {
            let len = children_len();
            if len == 0 {
                range_sig.set((0, 0));
            } else {
                let start = (child_start() as usize).min(len.saturating_sub(1));
                let end = (start + children_shown() as usize).min(len);
                range_sig.set((start, end));
            }
        });
    }

    // Scroll target into view when requested (moved after layout signals are defined)
    if let Some(scroll_sig) = scroll_to_index {
        Effect::new(move |_| {
            if let Some(target) = scroll_sig.get()
                && let Some(div) = scroller.get()
            {
                // approximate top of target row using measured prefix sums.
                // `effective_viewport` is read untracked so a viewport resize
                // does not re-trigger a scroll animation; before it became a
                // memo it was a plain constant and this effect only ever ran
                // in response to `scroll_sig`.
                //
                // Note: this drives `div.scrollTop`, so it is a no-op under
                // `ScrollSource::Window` where the div does not scroll.
                let viewport = effective_viewport.get_untracked();
                let row_top = target as f64 * row_height + fenwick.with(|f| f.sum(target));
                let current = div.scroll_top();
                let visible_top = current + header_h;
                let visible_bottom = current + header_h + viewport;
                let row_bottom = row_top + avg_row_height();
                let bottom_pad = 16.0;
                // decide desired scrollTop
                let desired = if row_top < visible_top - 1.0 {
                    (row_top - header_h).max(0.0)
                } else if row_bottom > visible_bottom + 1.0 {
                    (row_bottom - (header_h + viewport) + bottom_pad).max(0.0)
                } else {
                    current
                };
                // smooth scroll when we actually need to move
                if (desired - current).abs() > 0.5 {
                    if let Some(w) = window() {
                        let start_time = Rc::new(RefCell::new(None::<f64>));
                        let from = current;
                        let to = desired;
                        let dur = 200.0; // ms
                        type Callback = Closure<dyn FnMut(f64)>;
                        let cb_ref: Rc<RefCell<Option<Callback>>> = Rc::new(RefCell::new(None));
                        let cb_ref_clone = cb_ref.clone();
                        let start_time_clone = start_time.clone();
                        let div_clone = div.clone();
                        *cb_ref.borrow_mut() = Some(Closure::wrap(Box::new(move |ts: f64| {
                            let mut st = start_time_clone.borrow_mut();
                            let s = st.get_or_insert(ts);
                            let t = ((ts - *s) / dur).clamp(0.0, 1.0);
                            // easeOutCubic
                            let ease = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
                            let val = from + (to - from) * ease;
                            div_clone.set_scroll_top(val.round());
                            if t < 1.0 {
                                if let Some(w) = window() {
                                    let _ = w.request_animation_frame(
                                        cb_ref_clone
                                            .borrow()
                                            .as_ref()
                                            .unwrap()
                                            .as_ref()
                                            .unchecked_ref(),
                                    );
                                }
                            } else {
                                // drop the closure to avoid leaks
                                cb_ref_clone.borrow_mut().take();
                            }
                        })
                            as Box<dyn FnMut(f64)>));
                        let _ = w.request_animation_frame(
                            cb_ref.borrow().as_ref().unwrap().as_ref().unchecked_ref(),
                        );
                    } else {
                        // fallback without rAF
                        div.set_scroll_top(desired.round());
                    }
                }
            }
        });
    }
    // Both are constant for the life of the component, so they stay plain
    // values rather than closures.
    //
    // Window mode carries no `overflow` at all: any overflow on an ancestor of
    // the `position: sticky` header would re-parent its scrollport to this div
    // and stop it sticking to the viewport. Callers needing horizontal scroll
    // must apply it inside the header/row views.
    let container_class = if is_window {
        "w-full"
    } else {
        "overflow-y-auto overflow-x-auto w-full will-change-scroll contain-paint forced-layer"
    };
    let container_style = if is_window {
        String::new()
    } else {
        format!("height: {}px;", viewport_height.ceil() as u32)
    };
    let virtual_children = Memo::new(move |_| {
        each.with(|children| {
            let array_size = children.len();
            if array_size == 0 {
                return Vec::new();
            }
            // make sure start + end doesn't go over the length of the vector, and render at least one row
            let start = (child_start() as usize).min(array_size.saturating_sub(1));
            let end = (start + children_shown() as usize).min(array_size);
            children[start..end]
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, child)| (start + i, child))
                .collect()
        })
    });
    view! {
        <div
            on:scroll=move |scroll| {
                let div = event_target::<HtmlDivElement>(&scroll);
                last_scroll.set(div.scroll_top() as i32);
                if !raf_pending.get_untracked() {
                    raf_pending.set(true);
                    let last_scroll = last_scroll;
                    let set_scroll_offset = set_scroll_offset;
                    let raf_pending = raf_pending;
                    if let Some(w) = window() {
                        let cb = Closure::wrap(Box::new(move |_: f64| {
                            set_scroll_offset(last_scroll.get_untracked());
                            raf_pending.set(false);
                        }) as Box<dyn FnMut(f64)>);
                        let _ = w.request_animation_frame(cb.as_ref().unchecked_ref());
                        cb.forget();
                    } else {
                        // non-browser or fallback
                        set_scroll_offset(last_scroll.get_untracked());
                        raf_pending.set(false);
                    }
                }
            }
            node_ref=scroller
            class=container_class
            style=container_style
        >
            {header_opt
                .map(|h| match source {
                    ScrollSource::Container { .. } => {
                        view! { <div class="sticky top-0 z-10">{h}</div> }.into_any()
                    }
                    ScrollSource::Window { sticky_offset } => {
                        view! {
                            <div
                                class="sticky z-10"
                                style=format!("top: {}px;", sticky_offset.round() as i32)
                            >
                                {h}
                            </div>
                        }
                            .into_any()
                    }
                })}
            <div
                node_ref=list
                class="overflow-y-hidden overflow-x-visible will-change-[transform] relative w-full contain-layout forced-layer"
                style=move || {
                    format!(
                        r#"height: {}px;"#,
                        {
                            let base = each.with(|children| children.len() as f64) * row_height;
                            let delta_total = fenwick.with(|f| f.sum(children_len()));
                            let bottom_pad = 16.0;
                            (base + delta_total + bottom_pad).ceil() as u32
                        },
                    )
                }>
                // offset for visible nodes
                <div style=move || {
                    format!(
                        "
            transform: translateY({}px);
            {}
          ",
                        {
                            let start = child_start() as usize;
                            let delta_before = fenwick.with(|f| f.sum(start));
                            let val = child_start() as f64 * row_height + delta_before;
                            val.max(0.0).round() as i32
                        },
                        row_min_width
                            .as_ref()
                            .map(|w| format!("min-width: {w};"))
                            .unwrap_or_default(),
                    )
                }>
                    <For
                        each=virtual_children
                        key=move |(_, t): &(usize, T)| key(t)
                        children=move |(idx, child)| {
                            let row = NodeRef::<leptos::html::Div>::new();
                            let height_deltas = height_deltas;
                            let fenwick = fenwick;
                            if variable_height {
                                let resize_observer =
                                    StoredValue::new_local(
                                        None::<(ResizeObserver, Closure<dyn FnMut()>)>,
                                    );
                                on_cleanup(move || {
                                    resize_observer.update_value(|handle| {
                                        if let Some((observer, _callback)) = handle.take() {
                                            observer.disconnect();
                                        }
                                    });
                                });

                                Effect::new(move |_| {
                                    if let Some(el) = row.get() {
                                        let measure_height = move |measured: f64| {
                                            let delta = measured - row_height;
                                            height_deltas.update_value(|v| {
                                                if idx < v.len() {
                                                    let old = v[idx];
                                                    if (old - delta).abs() > 0.5 {
                                                        v[idx] = delta;
                                                        // O(log n) update instead of rebuilding prefix sums
                                                        fenwick.update(|f| f.add(idx, delta - old));
                                                    }
                                                }
                                            });
                                        };

                                        measure_height(el.offset_height() as f64);

                                        if resize_observer.with_value(|observer| observer.is_none())
                                        {
                                            let observed_el = el.clone();
                                            let callback = Closure::wrap(Box::new(move || {
                                                measure_height(observed_el.offset_height() as f64);
                                            })
                                                as Box<dyn FnMut()>);

                                            if let Ok(observer) =
                                                ResizeObserver::new(callback.as_ref().unchecked_ref())
                                            {
                                                observer.observe(&el);
                                                resize_observer.set_value(Some((observer, callback)));
                                            }
                                        }
                                    }
                                });
                            }
                            view! {
                                <div
                                    node_ref=row
                                    class=move || {
                                        if variable_height {
                                            "content-auto contain-layout will-change-transform".to_string()
                                        } else {
                                            "content-visible contain-layout will-change-transform overflow-hidden".to_string()
                                        }
                                    }
                                    style=move || {
                                        if variable_height {
                                            String::new()
                                        } else {
                                            format!("height: {}px;", row_height.round() as u32)
                                        }
                                    }
                                >
                                    {view(child)}
                                </div>
                            }
                        }
                    />
                </div>
            </div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_viewport_ignores_window_height() {
        let s = ScrollSource::Container {
            viewport_height: 720.0,
        };
        assert_eq!(effective_viewport_for(s, 1080.0), 720.0);
    }

    #[test]
    fn window_viewport_subtracts_sticky_offset() {
        let s = ScrollSource::Window {
            sticky_offset: 76.0,
        };
        assert_eq!(effective_viewport_for(s, 900.0), 824.0);
    }

    #[test]
    fn window_viewport_never_negative() {
        // A short window with tall sticky chrome must not produce a
        // negative viewport, which would make children_shown underflow.
        let s = ScrollSource::Window {
            sticky_offset: 200.0,
        };
        assert_eq!(effective_viewport_for(s, 120.0), 0.0);
    }

    const ROW_H: f64 = 32.0;
    const HEADER_H: f64 = 40.0;
    const WINDOW: ScrollSource = ScrollSource::Window {
        sticky_offset: 76.0,
    };

    #[test]
    fn ssr_fallback_height_round_trips_to_the_fallback_row_count() {
        // The pre-hydration height only protects hydration if it maps back
        // through the row math to exactly SSR_FALLBACK_ROWS rows.
        let fallback = viewport_px(WINDOW, 0.0, false, ROW_H, HEADER_H);
        assert_eq!(
            rows_for_viewport(fallback, ROW_H, 0),
            SSR_FALLBACK_ROWS as u32
        );
    }

    #[test]
    fn window_mode_ignores_a_measurement_until_hydrated() {
        // This is the whole hydration guard: the client can already read
        // innerHeight during its first render pass, and using it would render
        // a different row count than the server did.
        assert_eq!(
            viewport_px(WINDOW, 1080.0, false, ROW_H, HEADER_H),
            viewport_px(WINDOW, 0.0, false, ROW_H, HEADER_H),
        );
    }

    #[test]
    fn window_mode_uses_the_measurement_once_hydrated() {
        assert_eq!(
            viewport_px(WINDOW, 1080.0, true, ROW_H, HEADER_H),
            1080.0 - 76.0 - HEADER_H,
        );
    }

    #[test]
    fn window_mode_falls_back_when_hydrated_without_a_measurement() {
        // innerHeight can legitimately read 0 (background tab, some embeds).
        // Falling through would yield a 0px viewport, i.e. a single row.
        assert_eq!(
            viewport_px(WINDOW, 0.0, true, ROW_H, HEADER_H),
            SSR_FALLBACK_ROWS as f64 * ROW_H,
        );
    }

    #[test]
    fn container_mode_is_unaffected_by_the_hydration_gate() {
        // The 7 existing call sites must keep their exact previous geometry:
        // viewport minus header, whatever the hydration state or window size.
        let c = ScrollSource::Container {
            viewport_height: 720.0,
        };
        assert_eq!(viewport_px(c, 0.0, false, ROW_H, HEADER_H), 680.0);
        assert_eq!(viewport_px(c, 1080.0, true, ROW_H, HEADER_H), 680.0);
    }

    #[test]
    fn rows_for_viewport_matches_the_previous_arithmetic() {
        // Guards the extraction from `children_shown`: ceil, floor of 1, then
        // overscan added on top.
        assert_eq!(rows_for_viewport(680.0, 32.0, 10), 32);
        assert_eq!(rows_for_viewport(0.0, 32.0, 10), 11);
    }
}
