//! `/list/:id` behind the `lists-sync` Labs toggle.
//!
//! Phase 1 ships the switch with an identical page on both sides so the
//! toggle, cookie and route wiring can be exercised end to end before the
//! local-first document exists. Phase 4 replaces `ListViewSync`.

use leptos::prelude::*;

use crate::global_state::labs::{LAB_LISTS_SYNC, use_lab};
use crate::routes::list_view::ListView;

/// The Labs page. Identical to `ListView` until Phase 4.
#[component]
pub fn ListViewSync() -> impl IntoView {
    view! { <ListView /> }
}

/// Picks the page for `/list/:id`. The `LABS` cookie is server-visible, so
/// the server and the hydrating client make the same choice.
#[component]
pub fn ListRoute() -> impl IntoView {
    let sync = use_lab(LAB_LISTS_SYNC);
    move || {
        if sync.get() {
            view! { <ListViewSync /> }.into_any()
        } else {
            view! { <ListView /> }.into_any()
        }
    }
}
