use cfg_if::cfg_if;
#[cfg(feature = "hydrate")]
use leptos::{ev::resize, portal::Portal};
use leptos::{html::Div, prelude::*};
#[cfg(feature = "hydrate")]
use leptos_use::{
    UseElementBoundingReturn, UseElementSizeReturn, UseEventListenerOptions, use_element_bounding,
    use_element_size, use_event_listener_with_options, use_timeout_fn, use_window,
    use_window_scroll,
};

#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
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
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = set_x;
        let _ = set_y;
    }

    cfg_if! {
        if #[cfg(feature = "hydrate")] {
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
        }
    }

    (x.into(), y.into())
}

#[component]
pub fn Tooltip<T>(
    #[prop(into)]
    #[allow(unused_variables)]
    tooltip_text: Signal<String>,
    children: TypedChildrenFn<T>,
) -> impl IntoView
where
    T: Sized + Render + RenderHtml + Send,
{
    let (is_hovered, set_is_hovered) = signal(false);
    let (is_focused, set_is_focused) = signal(false);
    let target = NodeRef::<Div>::new();

    #[cfg(feature = "hydrate")]
    let timeout = std::sync::Arc::new(use_timeout_fn(
        move |_: ()| {
            set_is_hovered.set(false);
        },
        200.0,
    ));

    // Redefine helpers to avoid moving closures
    #[cfg(feature = "hydrate")]
    let stop_timeout = {
        let timeout = timeout.clone();
        move || (timeout.stop)()
    };
    #[cfg(feature = "hydrate")]
    let start_timeout = {
        let timeout = timeout.clone();
        move || (timeout.start)(())
    };

    let children = children.into_inner();
    let tooltip = {
        cfg_if! {
            if #[cfg(feature = "hydrate")] {
                let UseElementBoundingReturn {
                    bottom,
                    top,
                    left,
                    width,
                    ..
                } = use_element_bounding(target);

                // Capture necessary state for inner closure
                let timeout_inner = timeout.clone();

                move || {
                    (tooltip_text.with(|t| !t.is_empty()) && (is_hovered.get() || is_focused.get())).then({
                        let timeout_inner = timeout_inner.clone();

                        move || {
                            let (screen_width, _screen_height) = use_window_size();
                            // We don't need scroll_x for fixed positioning relative to viewport
                            let _ = use_window_scroll();
                            let node_ref = NodeRef::<Div>::new();
                            let UseElementSizeReturn {
                                width: tooltip_width,
                                height: tooltip_height,
                            } = use_element_size(node_ref);

                            let calculate_position = move || {
                                // If the tooltip hasn't been measured yet, hide it to prevent overlap/flashing
                                if tooltip_height() < 1.0 || tooltip_width() < 1.0 {
                                    return "opacity: 0; pointer-events: none; position: fixed;".to_string();
                                }

                                let element_center_x = left() + (width() / 2.0);

                                let gap = 5.0;
                                let mut pos_y = top() - tooltip_height() - gap;

                                if pos_y < 0.0 {
                                    pos_y = bottom() + gap;
                                }

                                let mut pos_x = element_center_x - (tooltip_width() / 2.0);
                                let max_x = screen_width() - tooltip_width() - 8.0;
                                pos_x = pos_x.clamp(8.0, max_x);

                                format!("top: {}px; left: {}px;", pos_y, pos_x)
                            };

                            let on_enter = {
                                let timeout = timeout_inner.clone();
                                move |_| {
                                    (timeout.stop)();
                                    set_is_hovered.set(true);
                                }
                            };

                            let on_leave = {
                                let timeout = timeout_inner.clone();
                                move |_| {
                                    (timeout.start)(());
                                }
                            };

                            view! {
                                <Portal mount=document().body().unwrap()>
                                    <div
                                        node_ref=node_ref
                                        class="fixed z-50 px-4 py-2 text-sm
                                        bg-gradient-to-br from-brand-950/95 to-brand-900/95
                                        border border-brand-800/50
                                        rounded-lg shadow-lg shadow-brand-950/50
                                        backdrop-blur-md
                                        text-gray-200
                                        transition-opacity duration-150
                                        animate-fade-in"
                                        style=calculate_position
                                        on:mouseenter=on_enter.clone()
                                        on:mouseleave=on_leave.clone()
                                    >
                                        {move || tooltip_text().to_string()}
                                    </div>
                                </Portal>
                            }.into_any()
                        }
                    })
                }
            } else {
                move || None::<AnyView>
            }
        }
    };

    view! {
        <div
            class="inline-block"
            // Clone handlers for outer view
            on:mouseenter=move |_| {
                #[cfg(feature = "hydrate")]
                {
                    stop_timeout();
                    set_is_hovered.set(true);
                }
            }
            on:mouseleave=move |_| {
                #[cfg(feature = "hydrate")]
                start_timeout();
            }
            on:focusin=move |_| set_is_focused.set(true)
            on:focusout=move |_| set_is_focused.set(false)
            node_ref=target
        >
            {children()}
            {tooltip}
        </div>
    }
}
