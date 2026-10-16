use leptos::prelude::use_context;

/// Server-provided Lists kill-switch state (`ULTROS_LISTS_READONLY`).
///
/// This is an operator switch, not a user preference: the value arrives in
/// the SSR bootstrap and there is intentionally no cookie, query override,
/// or Settings control that can change it. When it is set, list routes
/// render a read-only view over the REST projection and never open a list
/// document, so a misbehaving sync layer cannot write. The context being
/// absent (an older page shell) means editing is allowed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListsReadonly(pub bool);

/// Whether list editing is currently paused by the operator switch.
pub fn use_lists_readonly() -> bool {
    use_context::<ListsReadonly>().is_some_and(|flag| flag.0)
}
