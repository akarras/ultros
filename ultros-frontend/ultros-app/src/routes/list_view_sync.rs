//! `/list/:id` behind the `lists-sync` Labs toggle.
//!
//! Phase 1 ships the switch with an identical page on both sides so the
//! toggle, cookie and route wiring can be exercised end to end before the
//! local-first document exists. Phase 4 replaces `ListViewSync`.

use leptos::prelude::*;

use crate::global_state::labs::{LAB_LISTS_SYNC, use_lab};
use crate::routes::list_view::ListView;

/// The Labs page. Identical to `ListView` until Phase 4.
///
/// The wrapping div carries `data-testid="list-view-sync"` so tests (and
/// anyone checking a live page) can observe which branch of `ListRoute`
/// rendered — otherwise the two halves are indistinguishable. `display:
/// contents` keeps it invisible to layout; an inline style because Tailwind
/// only ships utility classes it sees used, and this marker has no class.
#[component]
pub fn ListViewSync() -> impl IntoView {
    view! {
        <div style="display:contents" data-testid="list-view-sync">
            <ListView />
        </div>
    }
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
