use crate::components::dismissable::use_dismiss_on_navigate;
use cfg_if::cfg_if;
use leptos::children::ViewFn;
use leptos::leptos_dom::helpers::{TimeoutHandle, set_timeout_with_handle};
#[cfg(feature = "hydrate")]
use leptos::portal::Portal;
use leptos::{html::Div, prelude::*};
#[cfg(feature = "hydrate")]
use leptos_use::{
    UseElementSizeReturn, UseEventListenerOptions, use_element_size,
    use_event_listener_with_options, use_window,
};
use std::time::Duration;

/// Anchor geometry in viewport coordinates (as returned by
/// `getBoundingClientRect`). The overlay is `position: fixed`, so all math in
/// this module stays in viewport space — no scroll offsets.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
pub(crate) struct AnchorRect {
    pub top: f64,
    pub left: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
pub(crate) struct OverlaySize {
    pub width: f64,
    pub height: f64,
}

/// Minimum distance kept between the overlay and every viewport edge.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
const EDGE_MARGIN: f64 = 8.0;
/// Gap between the anchor and the overlay.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
const ANCHOR_GAP: f64 = 8.0;

/// Compute the `(top, left)` for a fixed-position overlay anchored to
/// `anchor`: centered above it, flipped below when there is no room above,
/// clamped to the viewport on both axes.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
pub(crate) fn overlay_position(
    anchor: AnchorRect,
    overlay: OverlaySize,
    viewport: OverlaySize,
) -> (f64, f64) {
    // Prefer above the anchor; flip below when the overlay would clip the top.
    let mut top = anchor.top - overlay.height - ANCHOR_GAP;
    if top < EDGE_MARGIN {
        top = anchor.top + anchor.height + ANCHOR_GAP;
    }
    // `.max(EDGE_MARGIN)` keeps the clamp range valid when the overlay is
    // larger than the viewport (f64::clamp panics when min > max).
    let max_top = (viewport.height - overlay.height - EDGE_MARGIN).max(EDGE_MARGIN);
    let top = top.clamp(EDGE_MARGIN, max_top);

    let left = anchor.left + anchor.width / 2.0 - overlay.width / 2.0;
    let max_left = (viewport.width - overlay.width - EDGE_MARGIN).max(EDGE_MARGIN);
    let left = left.clamp(EDGE_MARGIN, max_left);

    (top, left)
}

/// Shared chrome for hover overlays: palette-driven gradient body, accent
/// hairline slot, glow shadow. Consumers append their own padding/sizing and
/// render `<AccentHairline/>` as their first child. Every color rides the
/// runtime brand CSS variables, so all palettes and light mode re-tint it.
pub(crate) const HOVER_CARD_CHROME: &str = "relative overflow-hidden rounded-lg \
    border border-brand-400/30 \
    bg-gradient-to-br from-brand-950/95 via-brand-900/90 to-brand-950/95 \
    backdrop-blur-md shadow-lg shadow-[color:var(--accent-glow)]";

/// 1px accent gradient across the top edge of a hover card.
#[component]
pub(crate) fn AccentHairline() -> impl IntoView {
    view! {
        <div class="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-[color:var(--accent)] to-transparent"></div>
    }
}

/// Hover/focus-triggered overlay primitive. Owns the portal, open/close state
/// (with optional open delay), and fixed positioning via [`overlay_position`].
/// No observers or listeners are created until the overlay actually opens.
#[component]
pub fn HoverCard<T>(
    /// Overlay content, rendered into a body portal while open.
    #[prop(into)]
    content: ViewFn,
    /// Milliseconds of sustained hover before opening. Focus opens instantly.
    #[prop(default = 0)]
    open_delay_ms: u32,
    /// While true, hover/focus never opens the overlay.
    #[prop(optional, into)]
    disabled: Signal<bool>,
    /// Classes for the anchor wrapper div.
    #[prop(optional, into)]
    class: Option<String>,
    children: TypedChildrenFn<T>,
) -> impl IntoView
where
    T: Sized + Render + RenderHtml + Send + 'static,
{
    let (hover_open, set_hover_open) = signal(false);
    let (is_focused, set_is_focused) = signal(false);
    // Pending open-delay timer (`TimeoutHandle` wraps an i32, so plain
    // sync storage is fine — `new_local`'s SendWrapper would panic when the
    // SSR arena drops it from a different tokio worker thread).
    let pending = StoredValue::new(None::<TimeoutHandle>);

    let clear_pending = move || {
        if let Some(handle) = pending.get_value() {
            handle.clear();
            pending.set_value(None);
        }
    };
    let request_open = move || {
        if disabled.get_untracked() {
            return;
        }
        if open_delay_ms == 0 {
            set_hover_open.set(true);
        } else if pending.get_value().is_none() {
            let handle = set_timeout_with_handle(
                move || {
                    pending.set_value(None);
                    set_hover_open.set(true);
                },
                Duration::from_millis(u64::from(open_delay_ms)),
            )
            .ok();
            pending.set_value(handle);
        }
    };

    // A navigation must never strand the overlay. `mouseleave` is the only
    // thing that closes a hover-opened card, and it does not fire when the
    // route change leaves the anchor in place (the item page keeps its hero
    // `HoverCard` across `/item/:world/:id` → `/item/:world/:other`) or when
    // it removes the anchor from under a cursor that never moved. Either way
    // the portal outlives the page it belonged to (#1283).
    use_dismiss_on_navigate(move || {
        clear_pending();
        // Guarded: `set` notifies whether or not the value changed, and a
        // page of item rows carries hundreds of closed cards whose overlay
        // closures would all re-run on every navigation for nothing.
        if hover_open.get_untracked() {
            set_hover_open.set(false);
        }
        if is_focused.get_untracked() {
            set_is_focused.set(false);
        }
    });

    let is_open = Signal::derive(move || !disabled.get() && (hover_open.get() || is_focused.get()));
    // Suppress unused warnings on the server build, where the overlay closure
    // below compiles to `None`.
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = is_open;
    }

    let target = NodeRef::<Div>::new();

    let overlay = {
        cfg_if! {
            if #[cfg(feature = "hydrate")] {
                let read_anchor_rect = move || {
                    target
                        .get_untracked()
                        .map(|el| {
                            let rect = el.get_bounding_client_rect();
                            AnchorRect {
                                top: rect.top(),
                                left: rect.left(),
                                width: rect.width(),
                                height: rect.height(),
                            }
                        })
                        .unwrap_or_default()
                };
                move || {
                    is_open.get().then({
                        let content = content.clone();
                        move || {
                            let anchor_rect = RwSignal::new(read_anchor_rect());
                            // Track the anchor while open: any scroll (capture
                            // catches nested containers) or resize moves its
                            // viewport rect. Registered inside the overlay
                            // view, so everything is dropped on close.
                            let _ = use_event_listener_with_options(
                                use_window(),
                                leptos::ev::scroll,
                                move |_| anchor_rect.set(read_anchor_rect()),
                                UseEventListenerOptions::default().capture(true).passive(true),
                            );
                            let _ = use_event_listener_with_options(
                                use_window(),
                                leptos::ev::resize,
                                move |_| anchor_rect.set(read_anchor_rect()),
                                UseEventListenerOptions::default().capture(false).passive(true),
                            );
                            // Escape closes a hover-opened overlay too: keydown
                            // fires on the focused element (usually `body`),
                            // never on the merely-hovered anchor, so the
                            // anchor-level handler can't catch this case.
                            let _ = use_event_listener_with_options(
                                use_window(),
                                leptos::ev::keydown,
                                move |ev| {
                                    if ev.key() == "Escape" {
                                        set_hover_open.set(false);
                                        set_is_focused.set(false);
                                    }
                                },
                                UseEventListenerOptions::default().capture(false).passive(true),
                            );
                            let node_ref = NodeRef::<Div>::new();
                            let UseElementSizeReturn {
                                width: overlay_width,
                                height: overlay_height,
                            } = use_element_size(node_ref);
                            let style = move || {
                                let overlay = OverlaySize {
                                    width: overlay_width.get(),
                                    height: overlay_height.get(),
                                };
                                let viewport = OverlaySize {
                                    width: window()
                                        .inner_width()
                                        .ok()
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or_default(),
                                    height: window()
                                        .inner_height()
                                        .ok()
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or_default(),
                                };
                                let (top, left) =
                                    overlay_position(anchor_rect.get(), overlay, viewport);
                                // Keep hidden until measured so the first
                                // paint can't flash at the wrong position.
                                let visibility =
                                    if overlay.width == 0.0 && overlay.height == 0.0 {
                                        "visibility: hidden;"
                                    } else {
                                        ""
                                    };
                                format!("top: {top}px; left: {left}px; {visibility}")
                            };
                            view! {
                                <Portal mount=document().body().unwrap()>
                                    <div
                                        node_ref=node_ref
                                        role="tooltip"
                                        class="fixed z-50 transition-opacity duration-150 animate-fade-in"
                                        style=style
                                    >
                                        {content.run()}
                                    </div>
                                </Portal>
                            }
                            .into_any()
                        }
                    })
                }
            } else {
                {
                    let _ = content;
                    move || None::<AnyView>
                }
            }
        }
    };

    let children = children.into_inner();
    view! {
        <div
            class=class.unwrap_or_default()
            on:mouseenter=move |_| request_open()
            on:mouseleave=move |_| {
                clear_pending();
                set_hover_open.set(false);
            }
            on:focusin=move |_| set_is_focused.set(true)
            on:focusout=move |_| set_is_focused.set(false)
            on:keydown=move |ev| {
                if ev.key() == "Escape" {
                    clear_pending();
                    set_hover_open.set(false);
                    set_is_focused.set(false);
                }
            }
            node_ref=target
        >
            {children()}
            {overlay}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: OverlaySize = OverlaySize {
        width: 1280.0,
        height: 800.0,
    };
    const OVERLAY: OverlaySize = OverlaySize {
        width: 200.0,
        height: 100.0,
    };

    fn anchor(top: f64, left: f64) -> AnchorRect {
        AnchorRect {
            top,
            left,
            width: 40.0,
            height: 20.0,
        }
    }

    #[test]
    fn hover_card_positions_above_and_centered_when_there_is_room() {
        let (top, left) = overlay_position(anchor(400.0, 600.0), OVERLAY, VIEWPORT);
        // 8px above the anchor, horizontally centered on it.
        assert_eq!(top, 400.0 - 100.0 - 8.0);
        assert_eq!(left, 600.0 + 20.0 - 100.0);
    }

    #[test]
    fn hover_card_flips_below_when_no_room_above() {
        let (top, _) = overlay_position(anchor(50.0, 600.0), OVERLAY, VIEWPORT);
        assert_eq!(top, 50.0 + 20.0 + 8.0);
    }

    #[test]
    fn hover_card_clamps_to_left_edge() {
        let (_, left) = overlay_position(anchor(400.0, 4.0), OVERLAY, VIEWPORT);
        assert_eq!(left, 8.0);
    }

    #[test]
    fn hover_card_clamps_to_right_edge() {
        let (_, left) = overlay_position(anchor(400.0, 1270.0), OVERLAY, VIEWPORT);
        assert_eq!(left, 1280.0 - 200.0 - 8.0);
    }

    #[test]
    fn hover_card_flipped_overlay_near_bottom_is_clamped() {
        // Anchor near the top forces a flip below; the short viewport then
        // forces the vertical clamp so the overlay never overflows the bottom.
        let viewport = OverlaySize {
            width: 1280.0,
            height: 160.0,
        };
        let (top, _) = overlay_position(anchor(40.0, 600.0), OVERLAY, viewport);
        assert_eq!(top, 160.0 - 100.0 - 8.0);
    }

    #[test]
    fn hover_card_tiny_viewport_does_not_panic_and_pins_to_margin() {
        // Overlay bigger than the viewport: both clamp ranges collapse to the
        // edge margin instead of panicking (f64::clamp panics when min > max).
        let viewport = OverlaySize {
            width: 100.0,
            height: 60.0,
        };
        let (top, left) = overlay_position(anchor(10.0, 10.0), OVERLAY, viewport);
        assert_eq!(top, 8.0);
        assert_eq!(left, 8.0);
    }
}
