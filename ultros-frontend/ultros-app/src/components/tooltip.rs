use cfg_if::cfg_if;
use leptos::{ev::resize, html::Div, *};
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
    let (x, set_x) = create_signal(initial_x);
    let (y, set_y) = create_signal(initial_y);

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
pub fn Tooltip(
    #[prop(into)] tooltip_text: MaybeSignal<Oco<'static, str>>,
    children: Box<dyn Fn() -> Fragment>,
) -> impl IntoView {
    let is_hover = create_rw_signal(false);
    let tooltip_text = match tooltip_text {
        MaybeSignal::Static(s) => Signal::derive(move || s.clone()),
        MaybeSignal::Dynamic(d) => d,
    };
    let target = create_node_ref::<Div>();
    let UseElementBoundingReturn {
        bottom,
        top,
        left,
        right,
        width,
        height,
        ..
    } = use_element_bounding(target);

    use_window_scroll();

    let tooltip = {
        move || {
            (tooltip_text.with(|t| !t.is_empty()) &&
            is_hover.get()).then(move || {
                let (screen_width, screen_height) = use_window_size();
                let (scroll_x, scroll_y) = use_window_scroll();
                let node_ref = create_node_ref::<Div>();
                let UseElementSizeReturn { width: tooltip_width, height: tooltip_height } = use_element_size(node_ref);
                let desired_position = move || {
                    log::info!("screen_x: {} screen_y: {}", screen_width(), screen_height());
                    log::info!("scroll_x: {} scroll_y: {}", scroll_x(), scroll_y());
                    let bottom = bottom();
                    let top = top();
                    let left = left();
                    let right = right();
                    let scroll_y = scroll_y();
                    let scroll_x = scroll_x();
                    let screen_width = screen_width();
                    let screen_height = screen_height();
                    let screen_top = scroll_y;
                    let screen_bottom = scroll_y + screen_height;
                    let screen_right = scroll_x + screen_width;
                    let screen_left = scroll_x;
                    let top_pad = top - screen_top;
                    let bottom_pad = screen_bottom - bottom;
                    let left_pad = left - screen_left;
                    let right_pad = screen_right - right;
                    let width = width();
                    let height = height();
                    let half_height = height / 2.0;
                    let half_width = width / 2.0;
                    let x = (-left_pad + right_pad).clamp(-1.0, 1.0);
                    let y = (-top_pad + bottom_pad).clamp(-1.0, 1.0);
                    let distance = 25.0;
                    format!("top: {}px; left: {}px;",
                        ((y * distance) + (y * half_height) + top) + (tooltip_height() / 2.0),
                        (x * distance) + (x * half_width) + left - (tooltip_width() / 2.0))
                };
                view! {
                    <Portal>
                        <div
                            class="fixed bg-violet-950 rounded-xl p-2 z-50 min-w-20 max-w-96"
                            style=desired_position
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
            class="tooltip"
            on:mouseover=move |_| {
                is_hover.set(true);
            }

            on:mouseout=move |_| {
                is_hover.set(false);
            }

            node_ref=target
        >
            {children()}
            {tooltip}
        </div>
    }
}
