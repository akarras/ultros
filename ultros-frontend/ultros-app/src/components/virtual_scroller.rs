use leptos::prelude::*;
use std::hash::Hash;
use std::{cell::RefCell, rc::Rc};
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
    let children_len = Memo::new(move |_| each.with(|children| children.len()));
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
        // reset scroll so new dataset renders from top (e.g., search changes)
        set_scroll_offset(0);
    });

    // dataset reset handled by length change effect
    let scroller: NodeRef<leptos::html::Div> = match scroller_ref {
        Some(r) => r,
        None => NodeRef::<leptos::html::Div>::new(),
    };

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
    let effective_viewport = (viewport_height - header_h).max(0.0);
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
        ((effective_viewport / avg_row_height()).ceil() as u32).max(1) + render_ahead
    });

    // Scroll target into view when requested (moved after layout signals are defined)
    if let Some(scroll_sig) = scroll_to_index {
        Effect::new(move |_| {
            if let Some(target) = scroll_sig.get()
                && let Some(div) = scroller.get()
            {
                // approximate top of target row using measured prefix sums
                let row_top = target as f64 * row_height + fenwick.with(|f| f.sum(target));
                let current = div.scroll_top() as f64;
                let visible_top = current + header_h;
                let visible_bottom = current + header_h + effective_viewport;
                let row_bottom = row_top + avg_row_height();
                let bottom_pad = 16.0;
                // decide desired scrollTop
                let desired = if row_top < visible_top - 1.0 {
                    (row_top - header_h).max(0.0)
                } else if row_bottom > visible_bottom + 1.0 {
                    (row_bottom - (header_h + effective_viewport) + bottom_pad).max(0.0)
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
                            div_clone.set_scroll_top(val.round() as i32);
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
                        div.set_scroll_top(desired.round() as i32);
                    }
                }
            }
        });
    }
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
                last_scroll.set(div.scroll_top());
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
            class="overflow-y-auto overflow-x-visible w-full will-change-scroll contain-paint forced-layer"
            style=format!("height: {}px;", viewport_height.ceil() as u32)
        >
            {header_opt.map(|h| view! { <div class="sticky top-0 z-10 content-visible contain-content">{h}</div> })}
            <div
                class="overflow-y-hidden overflow-x-visible will-change-[transform] relative w-full contain-layout contain-paint content-visible forced-layer"
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
          ",
                        {
                            let start = child_start() as usize;
                            let delta_before = fenwick.with(|f| f.sum(start));
                            let val = child_start() as f64 * row_height + delta_before;
                            val.max(0.0).round() as i32
                        },
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
                                Effect::new(move |_| {
                                    if let Some(el) = row.get() {
                                        let measured = el.offset_height() as f64;
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
                                    }
                                });
                            }
                            view! {
                                <div
                                    node_ref=row
                                    class=move || {
                                        if variable_height {
                                            "content-auto contain-layout contain-paint will-change-transform".to_string()
                                        } else {
                                            "content-visible contain-layout contain-paint will-change-transform overflow-hidden".to_string()
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
