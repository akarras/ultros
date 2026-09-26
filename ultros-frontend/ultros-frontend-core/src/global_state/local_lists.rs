//! Bridge to the lists saved on this device.
//!
//! The device-list runtime (`ultros-app`'s `list_doc::guest`) is browser-only
//! and sits above the shared UI crates, so the add-to-list modals reach it
//! through this context rather than a crate dependency. `ultros-app` provides
//! it on the hydrate build; with no provider (SSR, a native test) the modals
//! offer account lists only, until the browser runtime is ready.

use std::future::Future;
use std::pin::Pin;

use leptos::prelude::*;
use ultros_api_types::list::ListItem;

/// A browser-local future: the runtime talks to IndexedDB, so nothing here is
/// `Send`. The error is the runtime's already-translated message.
pub type LocalResult<T> = Pin<Box<dyn Future<Output = Result<T, String>>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalListSummary {
    /// The `device:` id the runtime assigned.
    pub id: String,
    pub name: String,
}

impl LocalListSummary {
    /// The editor route for this list.
    pub fn href(&self) -> String {
        format!("/list/device/{}", self.id)
    }
}

/// Plain function pointers rather than closures so the value is
/// `Copy + Send + Sync` and can live in Leptos context; only the futures they
/// return are browser-local.
#[derive(Clone, Copy)]
pub struct LocalLists {
    /// The lists saved on this device that are not bound to an account list.
    /// A bound list is already offered among the account lists (or is
    /// mid-transfer), so offering it here too would add to it twice.
    pub list: fn() -> LocalResult<Vec<LocalListSummary>>,
    /// Creates a named, empty list on this device.
    pub create: fn(String) -> LocalResult<LocalListSummary>,
    /// Merges `items` into the list as one undo step and waits for the save.
    /// `ListItem::list_id` is ignored: a device list has no numeric id.
    pub add_items: fn(String, Vec<ListItem>) -> LocalResult<()>,
}

/// The bridge, when the browser runtime has been provided.
pub fn use_local_lists() -> Signal<Option<LocalLists>> {
    let bridge = use_context::<LocalLists>();
    Signal::derive(move || bridge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editor_link_needs_no_preview_flag() {
        let summary = LocalListSummary {
            id: "device:abc".into(),
            name: "Raid supplies".into(),
        };
        assert_eq!(summary.href(), "/list/device/device:abc");
    }

    /// The bridge must be storable in Leptos context, which requires
    /// `Send + Sync`; function pointers satisfy that where closures over the
    /// browser runtime could not.
    #[test]
    fn the_bridge_is_context_safe() {
        fn assert_context<T: Clone + Send + Sync + 'static>() {}
        assert_context::<LocalLists>();
    }
}
