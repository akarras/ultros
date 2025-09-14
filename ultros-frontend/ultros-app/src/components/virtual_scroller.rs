use leptos::prelude::*;
use std::hash::Hash;
use web_sys::HtmlDivElement;

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
) -> impl IntoView
where
    D: Fn(T) -> V + 'static + Clone + Send,
    V: IntoView + 'static,
    KF: Fn(&T) -> K + 'static + Clone + Send,
    K: Eq + Hash + 'static,
    T: 'static + Clone + Send + Sync,
{
    let render_ahead: u32 = if overscan == 0 { 10 } else { overscan };
    let header_h: f64 = header_height.max(0.0);
    let header_opt: Option<AnyView> = header;
    let (scroll_offset, set_scroll_offset) = signal(0);
    // hybrid variable-height state: per-index delta from estimated row_height and prefix sums
    let children_len = Memo::new(move |_| each.with(|children| children.len()));
    let (height_deltas, set_height_deltas) = signal(Vec::<f64>::new());
    let initial_len = each.with(|children| children.len());
    let fenwick = RwSignal::new(Fenwick::new(initial_len));

    // keep vectors sized to item count and reinitialize Fenwick when the dataset changes
    Effect::new(move |_| {
        let len = children_len();
        // reset measurements on length change
        let v = vec![0.0; len];
        set_height_deltas.set(v);
        fenwick.update(|f| {
            f.reset(len);
        });
        // reset scroll so new dataset renders from top (e.g., search changes)
        set_scroll_offset(0);
    });

    // dataset reset handled by length change effect

    // use memo here so our signals only retrigger if the value actually changed.
    let child_start = Memo::new(move |_| {
        let len = children_len();
        let _ = each.with(|_| ());
        if len == 0 {
            return 0u32;
        }
        // binary search for smallest i where i*row_height + prefix_sums[i] >= effective_scroll
        let effective_scroll = (scroll_offset() as f64 - header_h).max(0.0);
        let mut lo: i32 = 0;
        let mut hi: i32 = len as i32;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let base = mid as f64 * row_height;
            let delta = fenwick.with(|f| f.sum(mid as usize));
            if base + delta < effective_scroll {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        {
            let lo_u32 = (lo.max(0) as u32);
            lo_u32.saturating_sub(render_ahead / 2)
        }
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
    let children_shown =
        ((effective_viewport / avg_row_height()).ceil() as u32).max(1) + render_ahead;

    let virtual_children = Memo::new(move |_| {
        each.with(|children| {
            let array_size = children.len();
            if array_size == 0 {
                return Vec::new();
            }
            // make sure start + end doesn't go over the length of the vector, and render at least one row
            let start = (child_start() as usize).min(array_size.saturating_sub(1));
            let end = (start + children_shown as usize).min(array_size);
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
                set_scroll_offset(div.scroll_top());
            }

            style=format!(
                r#"
        height: {}px;
        overflow-y: auto;
        overflow-x: visible;
        width: 100%;
      "#,
                viewport_height.ceil() as u32,
            )
        >
            {header_opt.map(|h| view! { <div style="position: sticky; top: 0; z-index: 10;">{h}</div> })}
            <div style=move || {
                format!(
                    r#"
          height: {}px;
          overflow-y: hidden;
          overflow-x: visible;
          will-change: transform;
          position: relative;
          width: 100%;
        "#,
                    {
                        let base = each.with(|children| children.len() as f64) * row_height;
                        let delta_total = fenwick.with(|f| f.sum(children_len()));
                        (base + delta_total).ceil() as u32
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
                            (val.max(0.0).round() as i32)
                        },
                    )
                }>
                    <For
                        each=virtual_children
                        key=move |(_, t): &(usize, T)| key(t)
                        children=move |(idx, child)| {
                            let row = NodeRef::<leptos::html::Div>::new();
                            let set_height_deltas = set_height_deltas.clone();
                            let height_deltas = height_deltas.clone();
                            let fenwick = fenwick.clone();
                            Effect::new(move |_| {
                                if let Some(el) = row.get() {
                                    let measured = el.offset_height() as f64;
                                    let delta = measured - row_height;
                                    let mut v = height_deltas.get_untracked();
                                    if idx < v.len() {
                                        let old = v[idx];
                                        if (old - delta).abs() > 0.5 {
                                            v[idx] = delta;
                                            set_height_deltas.set(v.clone());
                                            // O(log n) update instead of rebuilding prefix sums
                                            fenwick.update(|f| f.add(idx, delta - old));
                                        }
                                    }
                                }
                            });
                            view! { <div node_ref=row>{view(child)}</div> }
                        }
                    />
                </div>
            </div>
        </div>
    }
    .into_any()
}
