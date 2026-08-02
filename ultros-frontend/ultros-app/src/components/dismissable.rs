//! Shared dismissal wiring for toggle-button popovers.
//!
//! Extracted from `GroupedNavPopover`, the reference implementation of the
//! idiom: a popover closes on route change, on a click/tap outside its
//! container, and on Escape. A menu whose only way to close is re-tapping
//! its own trigger is a bug on mobile, where there is no hover state to
//! hint at that (#1056, the overlays bullet of #1068).

use leptos::html::Div;
use leptos::prelude::*;
use leptos_router::hooks::use_location;

/// Wires route-change, outside-click, and Escape dismissal onto the
/// popover container behind `container`. `dismiss` must close every
/// popover anchored inside that container.
///
/// Outside-click and Escape are hydrate-only: there is no document to
/// listen to on the server, and the same gate is used for
/// `use_element_hover` elsewhere in this codebase. The route-change
/// `Effect` only runs on the client, so it can't desync SSR.
pub fn use_dismissable(
    container: NodeRef<Div>,
    dismiss: impl Fn() + Clone + Send + Sync + 'static,
) {
    // Close after navigation — link clicks change the route, not the
    // popover's own state.
    let location = use_location();
    let pathname = location.pathname;
    {
        let dismiss = dismiss.clone();
        Effect::new(move |_| {
            pathname.track();
            dismiss();
        });
    }

    #[cfg(feature = "hydrate")]
    {
        let _ = leptos_use::on_click_outside(container, {
            let dismiss = dismiss.clone();
            move |_| dismiss()
        });
        // Escape works from anywhere inside the container (the event
        // bubbles up from whichever child has focus).
        let _ = leptos_use::use_event_listener(container, leptos::ev::keydown, move |ev| {
            if ev.key() == "Escape" {
                dismiss();
            }
        });
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = container;
        let _ = dismiss;
    }
}
