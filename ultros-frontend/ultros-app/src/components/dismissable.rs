//! Shared dismissal wiring for popovers and hover overlays.
//!
//! Extracted from the item explorer's `GroupedNavPopover`, which was the
//! reference implementation of the idiom: a popover closes on route change,
//! on a click/tap outside its container, and on Escape. A menu whose only way
//! to close is re-tapping its own trigger is a bug on mobile, where there is
//! no hover state to hint at that (#1056, the overlays bullet of #1068).
//!
//! That popover has since been replaced by an in-flow accordion
//! (`GroupedNavAccordion`), which needs none of this — it is not an overlay,
//! so there is nothing to tap away from. The helper lives on for the seven
//! call sites that are still overlays.
//!
//! [`use_dismiss_on_navigate`] is the route-change half on its own, for
//! overlays that are not toggle buttons at all: the hover cards and sparkline
//! tooltips, which have their own open/close wiring but share the rule that a
//! navigation must never leave an overlay behind (#1283).

use crate::components::app_link::use_location_or_default;
use leptos::html::Div;
use leptos::prelude::*;

/// Close on route change, and nothing else.
///
/// Split out of [`use_dismissable`] so an overlay that owns the rest of its
/// wiring can still share the one dismissal rule that has nothing to do with
/// pointers. `HoverCard` is that caller: it opens on hover and closes on
/// `mouseleave`, so it has no container to click outside of, and a navigation
/// that leaves the anchor in place — or removes it from under a motionless
/// cursor — fires no `mouseleave` at all. The overlay then stays on screen,
/// anchored to nothing the user can hover away from (#1283).
///
/// Only the pathname is tracked, deliberately. The query string is where
/// `ControlBar` and `ChartToolbar` popovers write the filters they exist to
/// edit; closing them the moment a user picks one would be a worse bug than
/// the one this fixes.
///
/// Reads the location through [`use_location_or_default`] rather than
/// `use_location()`: this runs under every overlay in the app, including ones
/// rendered inside a suspended SSR fragment whose owner can be disposed before
/// it resolves, and `use_location()` is an `expect` in exactly that case (see
/// `components::app_link`). A missing router yields a pathname that never
/// changes, so the effect runs once and then never again.
pub fn use_dismiss_on_navigate(dismiss: impl Fn() + Send + Sync + 'static) {
    let pathname = use_location_or_default().pathname;
    Effect::new(move |_| {
        pathname.track();
        dismiss();
    });
}

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
    use_dismiss_on_navigate(dismiss.clone());

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
