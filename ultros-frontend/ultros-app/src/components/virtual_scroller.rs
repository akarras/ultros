use leptos::prelude::*;
use std::hash::Hash;
use web_sys::HtmlDivElement;

/// Virtual scroller currently mimics the API of the ForEach components, but adds a row_height and viewport_height.
/// It might be possible to not have a fixed row height in the future, but for now it's good enough!
///
/// ### Known issues:
/// Because it makes multiple divs to create the scrolling effect, it's currently not possible
/// to use this with tables that have a table header.
#[component]
pub fn VirtualScroller<T, D, V, KF, K>(
    each: Signal<Vec<T>>,
    key: KF,
    view: D,
    viewport_height: f64,
    row_height: f64,
) -> impl IntoView
where
    D: Fn(T) -> V + 'static + Clone + Send,
    V: IntoView + 'static,
    KF: Fn(&T) -> K + 'static + Clone + Send,
    K: Eq + Hash + 'static,
    T: 'static + Clone + Send + Sync,
{
    let render_ahead = 10;
    let (scroll_offset, set_scroll_offset) = signal(0);
    // use memo here so our signals only retrigger if the value actually changed.
    let child_start = Memo::new(move |_| {
        ((scroll_offset() as f64 / row_height) as u32).saturating_sub(render_ahead / 2)
    });
    let children_shown = (viewport_height / row_height).ceil() as u32 + render_ahead;
    // let _ = key; // temporary getting rid of unused variable warning without renaming the key.
    let virtual_children = move || {
        each.with(|children| {
            let array_size = children.len();
            // make sure start + end doesn't go over the length of the vector
            let start = (child_start() as usize).min(array_size);
            let end = (child_start() + children_shown).min(array_size as u32) as usize;
            children[start..end].to_vec()
        })
    };
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
                    (each.with(|children| children.len() + render_ahead as usize) as f64
                        * row_height)
                        .ceil() as u32,
                )
            }>
                // offset for visible nodes
                <div style=move || {
                    format!(
                        "
            transform: translateY({}px);
          ",
                        (child_start() as f64 * row_height) as u32,
                    )
                }>
                    // {move || virtual_children().into_iter().map(|child| view(child)).collect::<Vec<_>>()}
                    // For component currently has issues. Possibly
                    // https://github.com/leptos-rs/leptos/issues/533
                    <For each=virtual_children key=key children=view />
                </div>
            </div>
        </div>
    }
    .into_any()
}
