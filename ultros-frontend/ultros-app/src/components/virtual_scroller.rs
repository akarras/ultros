use leptos::prelude::*;
use std::hash::Hash;
use std::{cell::RefCell, rc::Rc};
#[cfg(feature = "hydrate")]
use web_sys::ResizeObserver;
use web_sys::wasm_bindgen::JsCast;
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::{HtmlDivElement, window};

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
    // Only called from the hydrate-gated ResizeObserver effect; the SSR
    // build never measures rows, so the tree is only ever read there.
    #[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
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

/// Rows to render for a viewport of `viewport` px, including overscan.
pub(crate) fn rows_for_viewport(viewport: f64, avg_row_height: f64, overscan: u32) -> u32 {
    ((viewport / avg_row_height.max(1.0)).ceil() as u32)
        .max(1)
        .saturating_add(overscan)
}

/// The first row to render for a scroll offset of `effective_scroll` px past
/// the header: a binary search for the smallest `i` whose top edge is at or
/// past that offset, then half the overscan rendered above it.
///
/// `prefix_delta(i)` is the measured height difference of rows `0..i` against
/// `row_height` (the Fenwick prefix sum); it is `0.0` for a fixed-height
/// list. Pulled out of the memo so the row a scroll position maps to can be
/// tested without a DOM — `routes::recipe_analyzer`'s window test does, since
/// its lazy fetch keys on the published range.
pub(crate) fn first_visible_row(
    len: usize,
    row_height: f64,
    effective_scroll: f64,
    prefix_delta: impl Fn(usize) -> f64,
    overscan: u32,
) -> u32 {
    let mut lo: i32 = 0;
    let mut hi: i32 = len as i32;
    while lo < hi {
        let mid = (lo + hi) / 2;
        let base = mid as f64 * row_height;
        if base + prefix_delta(mid as usize) < effective_scroll {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    (lo.max(0) as u32).saturating_sub(overscan / 2)
}

/// The rendered row range `(start, end)` published to a parent's
/// `visible_range`, `end` exclusive: the scroller's first rendered row and
/// the rows it renders, both clamped to the data it actually has. A parent
/// fetches for exactly this slice (plus its own prefetch margin), so the
/// clamping is what keeps a short table from asking for rows that do not
/// exist and a long one from asking for all of them.
pub(crate) fn rendered_range(first: usize, shown: usize, len: usize) -> (usize, usize) {
    if len == 0 {
        return (0, 0);
    }
    let start = first.min(len - 1);
    (start, start.saturating_add(shown).min(len))
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
    row_height: f64,
    #[prop(optional, into)] header: Option<AnyView>,
    #[prop(optional)] header_height: f64,
    #[prop(optional)] overscan: u32,
    #[prop(optional)] variable_height: bool,
    #[prop(optional, into)] scroll_to_index: Option<Signal<Option<usize>>>,
    #[prop(optional)] scroller_ref: Option<NodeRef<leptos::html::Div>>,
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
        // A changed result count starts the container at the beginning.
        set_scroll_offset(0);
    });

    // dataset reset handled by length change effect
    let scroller: NodeRef<leptos::html::Div> = match scroller_ref {
        Some(r) => r,
        None => NodeRef::<leptos::html::Div>::new(),
    };
    let list = NodeRef::<leptos::html::Div>::new();

    // use memo here so our signals only retrigger if the value actually changed.
    let child_start = Memo::new(move |_| {
        let len = children_len();
        each.with(|_| ());
        if len == 0 {
            return 0u32;
        }
        let effective_scroll = (scroll_offset() as f64 - header_h).max(0.0);

        fenwick.with(|f| {
            first_visible_row(
                len,
                row_height,
                effective_scroll,
                |i| f.sum(i),
                render_ahead,
            )
        })
    });
    let effective_viewport = Memo::new(move |_| (viewport_height - header_h).max(0.0));
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
            // The empty case reads neither signal, so an empty list does not
            // subscribe this effect to the scroll position.
            let range = if len == 0 {
                (0, 0)
            } else {
                rendered_range(child_start() as usize, children_shown() as usize, len)
            };
            range_sig.set(range);
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
    let container_class =
        "overflow-y-auto overflow-x-auto w-full will-change-scroll contain-paint forced-layer";
    let container_style = format!("height: {}px;", viewport_height.ceil() as u32);
    // Only the outer container scrolls; a second scrollport clips wide rows.
    let list_class = "will-change-[transform] relative w-full contain-layout forced-layer";
    let virtual_children = Memo::new(move |_| {
        each.with(|children| {
            let array_size = children.len();
            if array_size == 0 {
                return Vec::new();
            }
            // make sure start + end doesn't go over the length of the vector, and render at least one row
            let (start, end) = rendered_range(
                child_start() as usize,
                children_shown() as usize,
                array_size,
            );
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
            {header_opt.map(|h| view! { <div class="sticky top-0 z-10">{h}</div> }.into_any())}
            // The full-height row area shares its parent's scrollport.
            <div
                node_ref=list
                class=list_class
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
                        // An empty value is treated exactly like `None`: a
                        // caller that forwards this prop through a component
                        // whose own prop is a plain `String` (the `#[component]`
                        // macro strips the `Option`, so `AnalyzerGrid`'s is)
                        // hands us `""` when *its* caller passed nothing, and
                        // `min-width: ;` is an invalid declaration.
                        row_min_width
                            .as_deref()
                            .filter(|w| !w.is_empty())
                            .map(|w| format!("min-width: {w};"))
                            .unwrap_or_default(),
                    )
                }>
                    <For
                        each=virtual_children
                        key=move |(_, t): &(usize, T)| key(t)
                        children={
                            let row_class = if variable_height {
                                "content-auto contain-layout will-change-transform"
                            } else {
                                "content-visible contain-layout will-change-transform overflow-hidden"
                            };
                            let row_style = if variable_height {
                                String::new()
                            } else {
                                format!("height: {}px;", row_height.round() as u32)
                            };

                            move |(idx, child)| {
                                let row = NodeRef::<leptos::html::Div>::new();
                                // Client-only for the same SendWrapper-on-SSR
                                // reason: browser observer cleanup must stay on the client.
                                #[cfg(not(feature = "hydrate"))]
                                let _ = idx;
                                #[cfg(feature = "hydrate")]
                                if variable_height {
                                    let height_deltas = height_deltas;
                                    let fenwick = fenwick;
                                    let resize_observer = StoredValue::new_local(
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
                                                // Hidden tabs and skipped content can report zero
                                                // before layout. Keep the estimate until measurable.
                                                if !measured.is_finite() || measured <= 0.0 {
                                                    return;
                                                }
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

                                            if resize_observer
                                                .with_value(|observer| observer.is_none())
                                            {
                                                let observed_el = el.clone();
                                                let callback = Closure::wrap(Box::new(move || {
                                                    measure_height(observed_el.offset_height() as f64);
                                                })
                                                    as Box<dyn FnMut()>);

                                                if let Ok(observer) = ResizeObserver::new(
                                                    callback.as_ref().unchecked_ref(),
                                                ) {
                                                    observer.observe(&el);
                                                    resize_observer
                                                        .set_value(Some((observer, callback)));
                                                }
                                            }
                                        }
                                    });
                                }
                                view! {
                                    <div
                                        node_ref=row
                                        class=row_class
                                        style=row_style.clone()
                                    >
                                        {view(child)}
                                    </div>
                                }
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

    /// The row area is the only element in the tree carrying
    /// `will-change-[transform]`, so it is findable without a DOM.
    fn row_area_class(html: &str) -> &str {
        let idx = html
            .find("will-change-[transform]")
            .unwrap_or_else(|| panic!("row area not rendered: {html}"));
        let start = html[..idx].rfind("class=\"").expect("class attribute") + 7;
        let end = start + html[start..].find('"').expect("class attribute closes");
        &html[start..end]
    }

    fn with_ssr_owner<F: FnOnce() -> String>(f: F) -> String {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(f)
    }

    fn container_html() -> String {
        with_ssr_owner(|| {
            view! {
                <VirtualScroller
                    each=Signal::derive(|| vec![1i32, 2, 3])
                    key=move |t: &i32| *t
                    view=move |t: i32| view! { <span>{t}</span> }
                    viewport_height=720.0
                    row_height=60.0
                />
            }
            .to_html()
        })
    }

    fn container_html_with_min_width(w: &'static str) -> String {
        with_ssr_owner(move || {
            view! {
                <VirtualScroller
                    each=Signal::derive(|| vec![1i32, 2, 3])
                    key=move |t: &i32| *t
                    view=move |t: i32| view! { <span>{t}</span> }
                    viewport_height=720.0
                    row_height=60.0
                    row_min_width=w
                />
            }
            .to_html()
        })
    }

    /// The shipped bug: in container mode the scroller div above already
    /// scrolls both axes, so an `overflow` pair here made the row area a
    /// second, never-scrolled horizontal scrollport. It clipped every row at
    /// the viewport width while the header — a sibling *outside* it — kept
    /// painting the full grid.
    #[test]
    fn container_mode_row_area_declares_no_overflow() {
        let class = {
            let html = container_html();
            row_area_class(&html).to_string()
        };
        assert!(
            !class.contains("overflow"),
            "container-mode row area must not be a scrollport: {class}"
        );
        // Everything else about the box is unchanged.
        assert!(
            class.contains("relative")
                && class.contains("w-full")
                && class.contains("contain-layout")
                && class.contains("forced-layer"),
            "{class}"
        );
    }

    #[test]
    fn spacer_sizes_itself_when_row_min_width_is_passed() {
        let html = container_html_with_min_width("max-content");
        assert!(html.contains("min-width: max-content;"), "{html}");
    }

    #[test]
    fn spacer_emits_no_min_width_when_the_prop_is_omitted_or_empty() {
        let omitted = container_html();
        assert!(
            !omitted.contains("min-width"),
            "an omitted prop must not size the spacer: {omitted}"
        );
        // `AnalyzerGrid` forwards a plain `String` (the macro strips the
        // `Option`), so a caller that passes nothing forwards `""` here.
        let empty = container_html_with_min_width("");
        assert!(
            !empty.contains("min-width"),
            "an empty prop must not emit `min-width: ;`: {empty}"
        );
    }

    #[test]
    fn rows_for_viewport_matches_the_previous_arithmetic() {
        // Guards the extraction from `children_shown`: ceil, floor of 1, then
        // overscan added on top.
        assert_eq!(rows_for_viewport(680.0, 32.0, 10), 32);
        assert_eq!(rows_for_viewport(0.0, 32.0, 10), 11);
    }

    #[test]
    fn degenerate_measurements_and_extreme_ranges_do_not_overflow() {
        assert_eq!(rows_for_viewport(680.0, 0.0, 10), 690);
        assert_eq!(rows_for_viewport(f64::MAX, 0.1, 10), u32::MAX);
        assert_eq!(rendered_range(5, usize::MAX, 10), (5, 10));
    }

    #[test]
    fn test_fenwick_tree_basic_operations() {
        let mut f = Fenwick::new(5);
        f.add(0, 10.0);
        f.add(1, 20.0);
        f.add(2, 30.0);

        assert_eq!(f.sum(0), 0.0);
        assert_eq!(f.sum(1), 10.0);
        assert_eq!(f.sum(2), 30.0);
        assert_eq!(f.sum(3), 60.0);
        assert_eq!(f.sum(4), 60.0);
        assert_eq!(f.sum(5), 60.0);
        assert_eq!(f.sum(6), 60.0); // OOB clamp
    }

    #[test]
    fn test_fenwick_tree_negative_delta() {
        let mut f = Fenwick::new(5);
        f.add(0, 10.0);
        f.add(1, 20.0);
        f.add(2, 30.0);

        // subtract 5 from index 1
        f.add(1, -5.0);

        assert_eq!(f.sum(0), 0.0);
        assert_eq!(f.sum(1), 10.0);
        assert_eq!(f.sum(2), 25.0);
        assert_eq!(f.sum(3), 55.0);
    }

    #[test]
    fn test_fenwick_tree_reset() {
        let mut f = Fenwick::new(3);
        f.add(0, 10.0);
        f.add(1, 10.0);

        assert_eq!(f.sum(2), 20.0);

        f.reset(5);
        assert_eq!(f.sum(2), 0.0);
        assert_eq!(f.n, 5);

        f.add(4, 5.0);
        assert_eq!(f.sum(5), 5.0);
    }

    #[test]
    fn test_virtual_scroller_binary_search_logic() {
        let n = 100;
        let mut f = Fenwick::new(n);
        let row_height = 20.0;

        // Let's set delta of 10.0 for items [10, 20)
        // This makes items 10-19 effectively 30px tall, rest 20px
        for i in 10..20 {
            f.add(i, 10.0);
        }

        // This mimics the child_start binary search
        // Calls the production search rather than mimicking it — a copy
        // of the loop here would let the two drift with both tests green.
        // Overscan 0 makes `first_visible_row` exactly this search.
        let find_first_gte =
            |scroll: f64| first_visible_row(n, row_height, scroll, |i| f.sum(i), 0);

        // Before modified items
        assert_eq!(find_first_gte(100.0), 5); // 5 * 20 = 100
        assert_eq!(find_first_gte(101.0), 6); // 6 * 20 = 120 > 101

        // Start of modified items
        assert_eq!(find_first_gte(200.0), 10); // 10 * 20 = 200
        assert_eq!(find_first_gte(201.0), 11); // 11 * 20 + 10 = 230 > 201

        // End of modified items
        // Sum after 20 items: 10 * 20 + 10 * 30 = 200 + 300 = 500
        assert_eq!(find_first_gte(500.0), 20);
        assert_eq!(find_first_gte(501.0), 21); // 21 * 20 + 100 = 520 > 501
    }

    #[test]
    fn test_virtual_scroller_binary_search_negative_deltas() {
        let n = 10;
        let mut f = Fenwick::new(n);
        let row_height = 20.0;

        // Simulate a row that shrunk
        f.add(1, -5.0); // Item 1 is 15px tall

        // Calls the production search rather than mimicking it — a copy
        // of the loop here would let the two drift with both tests green.
        // Overscan 0 makes `first_visible_row` exactly this search.
        let find_first_gte =
            |scroll: f64| first_visible_row(n, row_height, scroll, |i| f.sum(i), 0);

        assert_eq!(find_first_gte(15.0), 1); // 1 * 20 = 20 > 15
        assert_eq!(find_first_gte(20.0), 1); // 1 * 20 = 20 >= 20
        assert_eq!(find_first_gte(35.0), 2); // 2 * 20 - 5 = 35 >= 35
        assert_eq!(find_first_gte(36.0), 3); // 3 * 20 - 5 = 55 > 36
    }
}
