use cfg_if::cfg_if;
#[cfg(feature = "hydrate")]
use leptos::{ev::resize, portal::Portal};
use leptos::{html::Div, prelude::*};
#[cfg(feature = "hydrate")]
use leptos_use::{
    UseElementBoundingReturn, UseEventListenerOptions, use_element_bounding,
    use_event_listener_with_options, use_window,
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

#[cfg(any(feature = "hydrate", test))]
fn tooltip_position(
    trigger: (f64, f64, f64, f64),
    size: (f64, f64),
    viewport: (f64, f64),
    obstacles: &[(f64, f64, f64, f64)],
) -> (f64, f64) {
    let (w, h) = size;
    let max_x = (viewport.0 - w - 8.0).max(8.0);
    let max_y = (viewport.1 - h - 8.0).max(8.0);
    let center_x = trigger.0 + (trigger.2 - trigger.0) / 2.0;
    let center_y = (trigger.1 + trigger.3) / 2.0;
    let candidates = [
        (center_x - w / 2.0, trigger.1 - h - 8.0),
        (center_x - w / 2.0, trigger.3 + 8.0),
        (trigger.2 + 8.0, center_y - h / 2.0),
        (trigger.0 - w - 8.0, center_y - h / 2.0),
    ];
    candidates
        .into_iter()
        .map(|(x, y)| {
            let px = x.clamp(8.0, max_x);
            let py = y.clamp(8.0, max_y);
            let overlap: f64 = obstacles
                .iter()
                .map(|&(left, top, right, bottom)| {
                    (right.min(px + w) - left.max(px)).max(0.0)
                        * (bottom.min(py + h) - top.max(py)).max(0.0)
                })
                .sum();
            // Prefer staying clear of triggers, then avoid viewport clipping.
            let clipping = (x - px).abs() * h + (y - py).abs() * w;
            ((px, py), overlap, clipping)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.2.total_cmp(&b.2)))
        .unwrap()
        .0
}

/// Shared by every tooltip in the application, including portalled content.
#[derive(Clone, Copy)]
pub struct ActiveTooltip(pub RwSignal<Option<uuid::Uuid>>);

#[component]
pub fn Tooltip<T>(
    #[prop(into)]
    #[allow(unused_variables)]
    tooltip_text: Signal<String>,
    #[prop(optional, into)] class: Option<String>,
    children: TypedChildrenFn<T>,
) -> impl IntoView
where
    T: Sized + Render + RenderHtml + Send,
{
    let active = expect_context::<ActiveTooltip>().0;
    let id = uuid::Uuid::new_v4();
    let close_timer = StoredValue::new(None::<leptos::leptos_dom::helpers::TimeoutHandle>);
    let cancel_close = move || {
        let _ = close_timer.try_update_value(|timer| {
            if let Some(timer) = timer.take() {
                timer.clear();
            }
        });
    };
    let open = move || {
        cancel_close();
        if tooltip_text.with_untracked(|text| !text.is_empty()) {
            active.set(Some(id));
        }
    };
    let close = move || {
        if active.try_get_untracked() == Some(Some(id)) {
            active.set(None);
        }
    };
    let (is_hovered, set_is_hovered) = signal(false);
    let (is_focused, set_is_focused) = signal(false);
    // Suppress unused variable warnings for non-hydrate builds
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = is_hovered;
        let _ = is_focused;
    }
    let schedule_close = move || {
        cancel_close();
        let timer = set_timeout_with_handle(
            move || {
                if !is_hovered.get_untracked() && !is_focused.get_untracked() {
                    close();
                }
            },
            std::time::Duration::from_millis(180),
        )
        .ok();
        close_timer.set_value(timer);
    };
    on_cleanup(move || {
        cancel_close();
        close();
    });
    let target = NodeRef::<Div>::new();

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

                move || {
                    (tooltip_text.with(|t| !t.is_empty()) && active.get() == Some(id)).then(move || {
                        let (screen_width, screen_height) = use_window_size();
                        let node_ref = NodeRef::<Div>::new();
                        let UseElementBoundingReturn {
                            width: tooltip_width,
                            height: tooltip_height,
                            ..
                        } = use_element_bounding(node_ref);

                        let calculate_position = move || {
                            let w = tooltip_width();
                            let h = tooltip_height();
                            let mut obstacles = Vec::new();
                            if let Ok(nodes) = document().query_selector_all("[data-tooltip-trigger]") {
                                for index in 0..nodes.length() {
                                    use wasm_bindgen::JsCast;
                                    if let Some(element) = nodes.item(index).and_then(|node| node.dyn_into::<web_sys::Element>().ok()) {
                                        let rect = element.get_bounding_client_rect();
                                        obstacles.push((rect.left(), rect.top(), rect.right(), rect.bottom()));
                                    }
                                }
                            }
                            let (pos_x, pos_y) = tooltip_position(
                                (left(), top(), left() + width(), bottom()),
                                (w, h),
                                (screen_width(), screen_height()),
                                &obstacles,
                            );

                            format!("top: {}px; left: {}px;", pos_y, pos_x)
                        };

                        view! {
                            <Portal mount=document().body().unwrap()>
                                <div
                                    node_ref=node_ref
                                    role="tooltip"
                                    class="fixed z-50 px-4 py-2 text-sm
                                    bg-gradient-to-br from-brand-950/95 to-brand-900/95
                                    border border-brand-800/50
                                    rounded-lg shadow-lg shadow-brand-950/50
                                    backdrop-blur-md
                                    text-gray-200
                                    transition-opacity duration-150
                                    animate-fade-in"
                                    style=move || format!("{} max-width: calc(100vw - 16px); max-height: calc(100vh - 16px); overflow: auto;", calculate_position())
                                    on:mouseenter=move |_| { set_is_hovered.set(true); cancel_close(); }
                                    on:mouseleave=move |_| { set_is_hovered.set(false); schedule_close(); }
                                >
                                    {move || tooltip_text().to_string()}
                                </div>
                            </Portal>
                        }.into_any()
                    })
                }
            } else {
                move || None::<AnyView>
            }
        }
    };

    view! {
        <div
            class=move || {
                format!("inline-block {}", class.clone().unwrap_or_default())
            }
            attr:data-tooltip-trigger=""
            on:mouseenter=move |_| { set_is_hovered.set(true); open(); }
            on:mouseleave=move |_| { set_is_hovered.set(false); schedule_close(); }
            on:focusin=move |_| { set_is_focused.set(true); open(); }
            on:focusout=move |_| { set_is_focused.set(false); schedule_close(); }
            on:keydown=move |ev| {
                if ev.key() == "Escape" {
                    cancel_close();
                    close();
                    set_is_hovered.set(false);
                    set_is_focused.set(false);
                }
            }
            node_ref=target
        >
            {children()}
            {tooltip}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::tooltip_position;

    #[test]
    fn avoids_trigger_above_and_prefers_below() {
        let trigger = (100.0, 100.0, 120.0, 120.0);
        let above = (60.0, 50.0, 160.0, 95.0);
        assert_eq!(
            tooltip_position(trigger, (80.0, 40.0), (400.0, 300.0), &[trigger, above]),
            (70.0, 128.0)
        );
    }

    #[test]
    fn uses_side_when_both_vertical_placements_are_blocked() {
        let trigger = (100.0, 100.0, 120.0, 120.0);
        let obstacles = [
            trigger,
            (60.0, 50.0, 160.0, 95.0),
            (60.0, 125.0, 160.0, 180.0),
        ];
        assert_eq!(
            tooltip_position(trigger, (80.0, 40.0), (400.0, 300.0), &obstacles),
            (128.0, 90.0)
        );
    }

    #[test]
    fn stays_inside_small_viewport() {
        let (x, y) = tooltip_position((0.0, 0.0, 20.0, 20.0), (90.0, 70.0), (100.0, 80.0), &[]);
        assert_eq!((x, y), (8.0, 8.0));
    }
}
