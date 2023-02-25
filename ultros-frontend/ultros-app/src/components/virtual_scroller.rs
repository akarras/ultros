use leptos::*;
use std::hash::Hash;
use web_sys::HtmlDivElement;

#[component]
pub fn VirtualScroller<T, D, V, KF, K>(
    cx: Scope,
    each: Signal<Vec<T>>,
    key: KF,
    view: D,
    viewport_height: f64,
    row_height: f64,
) -> impl IntoView
where
    D: Fn(Scope, T) -> V + 'static,
    V: IntoView,
    KF: Fn(&T) -> K + 'static,
    K: Eq + Hash + 'static,
    T: 'static + Clone,
{
    let render_ahead = 0;
    let (scroll_offset, set_scroll_offset) = create_signal(cx, 0);
    let child_start =
        move || ((scroll_offset() as f64 / row_height) as u32).saturating_sub(render_ahead / 2);
    let children_shown = (viewport_height / row_height).ceil() as u32 + render_ahead;
    create_effect(cx, move |_| {});
    let virtual_children = move || {
        each.with(|children| {
            let array_size = children.len();
            // make sure start + end doesn't go over the length of the vector
            let start = child_start() as usize;
            let end = (child_start() + children_shown).min(array_size as u32) as usize;

            log::info!(
                "child start {}, scroll_top: {} {start} {end}",
                child_start(),
                scroll_offset()
            );
            children[start..end].to_vec()
        })
    };
    view! {cx,
        <div
        on:scroll= move |scroll| {
            let div = event_target::<HtmlDivElement>(&scroll);
            set_scroll_offset(div.scroll_top());
        }
      style=format!(r#"
        height: {}px;
        overflow: auto;
      "#, viewport_height.ceil() as u32)
    >
      <div

        style=move || {
            format!(r#"
          height: {}px;
          overflow: hidden;
          will-change: transform;
          position: relative;
        "#, (each.with(|children| children.len()) as f64 * row_height).ceil() as u32)}
      >
        <div // offset for visible nodes
          style=move || format!("
            transform: translateY({}px);
          ", (child_start() as f64 * row_height) as u32)
        >
          <For each=virtual_children
               key=key
               view=view
          />
        </div>
      </div>
    </div>
    }
}
