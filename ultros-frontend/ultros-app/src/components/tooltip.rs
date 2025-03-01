use cfg_if::cfg_if;
use leptos::{ev::resize, html::Div, portal::Portal, prelude::*};
use leptos_use::{
    use_element_bounding, use_element_size, use_event_listener_with_options, use_window,
    use_window_scroll, UseElementBoundingReturn, UseElementSizeReturn, UseEventListenerOptions,
};

fn use_window_size() -> (Signal<f64>, Signal<f64>) {
    cfg_if! { if #[cfg(feature = "ssr")] {
        let initial_x = 0.0;
        let initial_y = 0.0;
    } else {
        let initial_x = window().inner_width().unwrap_or_default().as_f64().unwrap_or_default();
        let initial_y = window().inner_height().unwrap_or_default().as_f64().unwrap_or_default();
    }}
    let (x, set_x) = signal(initial_x);
    let (y, set_y) = signal(initial_y);

    let _ = use_event_listener_with_options(
        use_window(),
        resize,
        move |_| {
            set_x.set(
                window()
                    .inner_width()
                    .unwrap_or_default()
                    .as_f64()
                    .unwrap_or_default(),
            );
            set_y.set(
                window()
                    .inner_height()
                    .unwrap_or_default()
                    .as_f64()
                    .unwrap_or_default(),
            );
        },
        UseEventListenerOptions::default()
            .capture(false)
            .passive(true),
    );

    (x.into(), y.into())
}

#[component]
pub fn Tooltip<T>(
    #[prop(into)] tooltip_text: Signal<String>,
    children: TypedChildrenFn<T>,
) -> impl IntoView
where
    T: Sized + Render + RenderHtml + Send,
{
    let is_hover = RwSignal::new(false);
    let target = NodeRef::<Div>::new();
    let UseElementBoundingReturn {
        bottom,
        top,
        left,
        width,
        ..
    } = use_element_bounding(target);

    use_window_scroll();
    let children = children.into_inner();
    let tooltip = {
        move || {
            (tooltip_text.with(|t| !t.is_empty()) && is_hover.get()).then(move || {
                let (screen_width, screen_height) = use_window_size();
                let (scroll_x, scroll_y) = use_window_scroll();
                let node_ref = NodeRef::<Div>::new();
                let UseElementSizeReturn {
                    width: tooltip_width,
                    height: tooltip_height,
                } = use_element_size(node_ref);

                let calculate_position = move || {
                    let element_center_x = left() + (width() / 2.0);
                    let viewport_right = scroll_x() + screen_width();
                    let _viewport_bottom = scroll_y() + screen_height();

                    // Default to showing above the element
                    let mut pos_y = top() - tooltip_height() - 8.0; // 8px offset
                    let mut pos_x = element_center_x - (tooltip_width() / 2.0);

                    // If tooltip would go above viewport, show below instead
                    if pos_y < scroll_y() {
                        pos_y = bottom() + 8.0;
                    }

                    // Prevent tooltip from going off-screen horizontally
                    pos_x = pos_x.clamp(
                        scroll_x() + 8.0,                       // Left boundary
                        viewport_right - tooltip_width() - 8.0, // Right boundary
                    );

                    format!("top: {}px; left: {}px;", pos_y, pos_x)
                };

                view! {
                    <Portal mount=document().body().unwrap()>
                        <div
                            node_ref=node_ref
                            class="fixed z-50 px-4 py-2 text-sm
                                  bg-gradient-to-br from-violet-950/95 to-violet-900/95
                                  border border-violet-800/50
                                  rounded-lg shadow-lg shadow-violet-950/50
                                  backdrop-blur-md
                                  text-gray-200
                                  transition-opacity duration-150
                                  animate-fade-in"
                            style=calculate_position
                        >
                            {move || tooltip_text().to_string()}
                        </div>
                    </Portal>
                }
            })
        }
    };

    view! {
        <div
            class="inline-block"
            on:mouseenter=move |_| is_hover.set(true)
            on:mouseleave=move |_| is_hover.set(false)
            node_ref=target
        >
            {children()}
            {tooltip}
        </div>
    }
}
