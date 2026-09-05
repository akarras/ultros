//! Shared dismissal wiring for toggle-button popovers.
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
//! ## Why outside-click is not enough on its own
//!
//! Outside-click asks one question: did the pointer land outside *this*
//! popover's container? For siblings that is the whole answer — the chart
//! toolbar's three menus each close when another's button is pressed,
//! because that button is outside their container.
//!
//! It answers nothing for a popover mounted *inside* another's container.
//! The control bar's `actions` slot holds the Views and Market menus, so
//! pressing one of those buttons is a click inside the bar: the bar's own
//! Columns / `+ Filter` popovers never hear it and stay open underneath.
//! [`PopoverGroup`] closes that hole by making membership explicit — every
//! popover under one container registers its closer, and opening any of
//! them closes the rest.

use std::sync::Arc;

use leptos::html::Div;
use leptos::prelude::*;
use leptos_router::hooks::use_location;

/// A member's closer: shared because [`PopoverGroup::close_others`] clones
/// the ones it is about to run out of the collection first.
type Dismiss = Arc<dyn Fn() + Send + Sync>;

/// A set of popovers that share a container and must not be open at once.
///
/// Provided by the container's component ([`provide_popover_group`]) and
/// joined automatically by every [`use_dismissable`] call rendered beneath
/// it, including ones inside view props the container did not write.
#[derive(Copy, Clone)]
pub struct PopoverGroup {
    /// One `(member id, closer)` per popover. Ids are assignment order, and
    /// members are never removed: a group lives exactly as long as the
    /// container that provided it, so every member outlives it too.
    members: StoredValue<Vec<(usize, Dismiss)>>,
}

impl Default for PopoverGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl PopoverGroup {
    pub fn new() -> Self {
        Self {
            members: StoredValue::new(Vec::new()),
        }
    }

    /// Register `dismiss` and hand back the token this member announces its
    /// openings with.
    pub fn join(&self, dismiss: impl Fn() + Send + Sync + 'static) -> PopoverToken {
        let id = self.members.with_value(|m| m.len());
        self.members
            .update_value(|m| m.push((id, Arc::new(dismiss))));
        PopoverToken {
            group: Some(*self),
            id,
        }
    }

    /// Close every member except `id`.
    fn close_others(&self, id: usize) {
        // Cloned out of the `StoredValue` before any of them runs: a closer
        // sets signals, which can render a view that joins the group, and
        // pushing to `members` while it is borrowed would panic.
        let others = self.members.with_value(|m| {
            m.iter()
                .filter(|(member, _)| *member != id)
                .map(|(_, dismiss)| Arc::clone(dismiss))
                .collect::<Vec<_>>()
        });
        for dismiss in others {
            dismiss();
        }
    }
}

/// One member's handle on its [`PopoverGroup`].
///
/// A popover rendered outside any group gets a token all the same — one
/// whose [`opening`](PopoverToken::opening) does nothing — so a component
/// used both inside and outside a control bar needs no branch.
#[derive(Copy, Clone)]
pub struct PopoverToken {
    group: Option<PopoverGroup>,
    id: usize,
}

impl PopoverToken {
    /// Call from the click handler that is about to open this popover;
    /// every other member of the group closes. Not called when the same
    /// handler is *closing* the popover — a click that puts the bar back to
    /// nothing open has no business touching anyone else.
    pub fn opening(&self) {
        if let Some(group) = self.group {
            group.close_others(self.id);
        }
    }
}

/// Start a [`PopoverGroup`] for everything rendered inside this component.
/// Call it before the container's own [`use_dismissable`], so the container
/// joins the group it provides.
pub fn provide_popover_group() -> PopoverGroup {
    let group = PopoverGroup::new();
    provide_context(group);
    group
}

/// Wires route-change, outside-click, and Escape dismissal onto the
/// popover container behind `container`. `dismiss` must close every
/// popover anchored inside that container.
///
/// Returns this popover's [`PopoverToken`]: if an enclosing component
/// provided a [`PopoverGroup`], the caller must call
/// [`opening`](PopoverToken::opening) when it opens so the group's other
/// popovers close. Outside a group the token is inert.
///
/// Outside-click and Escape are hydrate-only: there is no document to
/// listen to on the server, and the same gate is used for
/// `use_element_hover` elsewhere in this codebase. The route-change
/// `Effect` only runs on the client, so it can't desync SSR.
pub fn use_dismissable(
    container: NodeRef<Div>,
    dismiss: impl Fn() + Clone + Send + Sync + 'static,
) -> PopoverToken {
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
        let _ = leptos_use::use_event_listener(container, leptos::ev::keydown, {
            let dismiss = dismiss.clone();
            move |ev| {
                if ev.key() == "Escape" {
                    dismiss();
                }
            }
        });
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = container;
    }

    match use_context::<PopoverGroup>() {
        Some(group) => group.join(dismiss),
        None => PopoverToken { group: None, id: 0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point: opening one member closes the others, whatever
    /// their nesting. A member never closes itself — the caller has
    /// already set its own state by the time it announces the open.
    #[test]
    fn opening_one_member_closes_every_other() {
        let owner = Owner::new();
        owner.with(|| {
            let group = PopoverGroup::new();
            let bar = RwSignal::new(true);
            let views = RwSignal::new(false);
            let market = RwSignal::new(true);

            let bar_token = group.join(move || bar.set(false));
            let views_token = group.join(move || views.set(false));
            let _market_token = group.join(move || market.set(false));

            views.set(true);
            views_token.opening();
            assert!(views.get(), "a member must not close itself");
            assert!(!bar.get(), "the control bar's own popovers must close");
            assert!(!market.get(), "a sibling menu must close");

            bar.set(true);
            bar_token.opening();
            assert!(bar.get());
            assert!(!views.get());
        });
    }

    /// A popover rendered outside any control bar still gets a token, and
    /// using it is a no-op rather than a branch at every call site.
    #[test]
    fn a_token_without_a_group_is_inert() {
        let token = PopoverToken { group: None, id: 0 };
        token.opening();
    }
}
