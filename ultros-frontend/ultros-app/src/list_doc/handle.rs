//! The page's handle on its document (spec section 3.1): the document, its
//! undo manager, a revision signal that memos derive from, an outbox of
//! local commits for the socket, and persistence on a debounce.
//!
//! `ListDocHandle::open` must only ever run in browser code. It stores the
//! document and undo manager in `StoredValue::new_local`, which panics if
//! its owner is ever disposed from a thread other than the one that created
//! it — harmless on wasm (there is only one thread) but fatal under SSR,
//! where a page's reactive owner can be torn down on a different tokio
//! worker thread than the one that ran the handler (repo issue #1332). SSR
//! never has a browser document to open in the first place, so `open` is
//! simply never called there; nothing in this file gates the type away,
//! since Task 8's component needs `ListDocHandle` to exist on both halves.

use leptos::prelude::*;
use ultros_list_doc::{DocError, ListDocument, ListUndo, MetaSnapshot, RowSnapshot, Subscription};

use crate::list_doc::adapter::{self, Edit};
use crate::list_doc::store::{self, BrowserStorage};

#[cfg(feature = "hydrate")]
const SAVE_DEBOUNCE_MS: u32 = 500;

/// The pending-save timer's payload. A real `gloo_timers::callback::Timeout`
/// on the client; a unit placeholder under SSR, where `install_persistence`
/// never schedules one, so the field always exists but is only ever
/// populated on the client.
#[cfg(feature = "hydrate")]
type SaveTimer = gloo_timers::callback::Timeout;
#[cfg(not(feature = "hydrate"))]
type SaveTimer = ();

#[derive(Clone, Copy)]
pub struct ListDocHandle {
    pub user_id: i64,
    pub list_id: i32,
    doc: StoredValue<ListDocument, LocalStorage>,
    undo: StoredValue<ListUndo, LocalStorage>,
    // Held so the document keeps notifying; dropping unsubscribes.
    #[allow(dead_code)]
    subscriptions: StoredValue<Vec<Subscription>, LocalStorage>,
    save_timer: StoredValue<Option<SaveTimer>, LocalStorage>,
    /// Bumped on every change, local or remote.
    pub revision: RwSignal<u64>,
    /// Local commits waiting for the socket, in order.
    pub outbox: RwSignal<Vec<Vec<u8>>>,
    /// `RealtimeStatus` vocabulary: connecting, live, reconnecting, offline.
    pub status: RwSignal<String>,
    /// Last known `ListPermission` as `i16`, cached beside the snapshot.
    pub permission: RwSignal<i16>,
}

impl ListDocHandle {
    /// Load the browser's snapshot for this user+list, or start empty.
    /// Loro's default random peer id is used; nothing stores or reuses peer
    /// ids. Browser-only — see the module doc comment.
    pub fn open(user_id: i64, list_id: i32) -> Self {
        let loaded = store::load(&BrowserStorage, user_id, list_id);
        let doc = loaded
            .as_ref()
            .and_then(|l| ListDocument::from_snapshot(&l.snapshot).ok())
            .unwrap_or_default();
        let permission = loaded.map(|l| l.permission).unwrap_or(0);
        let undo = ListUndo::new(&doc);
        let revision = RwSignal::new(0u64);
        let outbox = RwSignal::new(Vec::new());
        let on_change = doc.on_change(move || revision.update(|r| *r += 1));
        let on_local = doc.on_local_update(move |bytes| {
            let bytes = bytes.to_vec();
            outbox.update(|queue| queue.push(bytes));
        });
        let handle = Self {
            user_id,
            list_id,
            doc: StoredValue::new_local(doc),
            undo: StoredValue::new_local(undo),
            subscriptions: StoredValue::new_local(vec![on_change, on_local]),
            save_timer: StoredValue::new_local(None),
            revision,
            outbox,
            status: RwSignal::new("connecting".to_string()),
            permission: RwSignal::new(permission),
        };
        handle.install_persistence();
        handle
    }

    pub fn with_doc<R>(&self, f: impl FnOnce(&ListDocument) -> R) -> R {
        self.doc.with_value(f)
    }

    pub fn rows(&self) -> Vec<RowSnapshot> {
        self.with_doc(|doc| doc.rows())
    }

    pub fn meta(&self) -> MetaSnapshot {
        self.with_doc(|doc| doc.meta())
    }

    pub fn version(&self) -> Vec<u8> {
        self.with_doc(|doc| doc.version())
    }

    /// One user action, one undo step. The document commits inside, which
    /// bumps `revision` and pushes the update onto `outbox`.
    pub fn apply(&self, edit: Edit) -> Result<(), DocError> {
        self.doc.with_value(|doc| {
            let mut result = Ok(());
            self.undo
                .update_value(|undo| result = adapter::apply(doc, undo, edit));
            result
        })
    }

    pub fn import(&self, bytes: &[u8]) -> Result<(), DocError> {
        self.with_doc(|doc| doc.import(bytes).map(|_| ()))
    }

    pub fn export_since(&self, version: &[u8]) -> Result<Vec<u8>, DocError> {
        self.with_doc(|doc| doc.export_since(version))
    }

    pub fn is_ahead_of(&self, version: &[u8]) -> bool {
        self.with_doc(|doc| doc.is_ahead_of(version))
    }

    pub fn undo(&self) -> bool {
        let mut done = false;
        self.undo
            .update_value(|undo| done = undo.undo().unwrap_or(false));
        done
    }

    pub fn redo(&self) -> bool {
        let mut done = false;
        self.undo
            .update_value(|undo| done = undo.redo().unwrap_or(false));
        done
    }

    pub fn set_status(&self, status: &str) {
        self.status.set(status.to_string());
    }

    pub fn remember_permission(&self, permission: i16) {
        self.permission.set(permission);
        let _ = store::remember_permission(
            &BrowserStorage,
            self.user_id,
            self.list_id,
            permission,
            store::now_ms(),
        );
    }

    pub fn save_now(&self) {
        let snapshot = self.with_doc(|doc| doc.export_snapshot());
        if let Ok(snapshot) = snapshot {
            let _ = store::save(
                &BrowserStorage,
                self.user_id,
                self.list_id,
                &snapshot,
                self.permission.get_untracked(),
                store::now_ms(),
            );
        }
    }

    /// Drop this user's cached snapshot and permission for this list, and
    /// reset the in-memory document to empty. For the "forbidden / deleted"
    /// path: the server has said this list is no longer readable, so the
    /// stale local copy must not resurface on a later `open`.
    pub fn purge(&self) {
        store::purge(&BrowserStorage, self.user_id, self.list_id);
        let fresh = ListDocument::new();
        let undo = ListUndo::new(&fresh);
        let revision = self.revision;
        let outbox = self.outbox;
        let on_change = fresh.on_change(move || revision.update(|r| *r += 1));
        let on_local = fresh.on_local_update(move |bytes| {
            let bytes = bytes.to_vec();
            outbox.update(|queue| queue.push(bytes));
        });
        self.doc.set_value(fresh);
        self.undo.set_value(undo);
        self.subscriptions.set_value(vec![on_change, on_local]);
        self.permission.set(0);
        self.revision.update(|r| *r += 1);
    }

    /// Stop this handle from doing any more background work: cancels the
    /// pending save timer and drops the document subscriptions. Call when a
    /// page navigates away from this list.
    pub fn close(&self) {
        self.save_timer.update_value(|timer| *timer = None);
        self.subscriptions.update_value(|subs| subs.clear());
    }

    /// Save half a second after the last change, and immediately when the
    /// tab is hidden. Client only: the server has no storage to write.
    #[cfg(feature = "hydrate")]
    fn install_persistence(&self) {
        let handle = *self;
        Effect::new(move |_| {
            let _ = handle.revision.get();
            // Dropping the previous timeout cancels it.
            handle.save_timer.update_value(|timer| *timer = None);
            let timeout = gloo_timers::callback::Timeout::new(SAVE_DEBOUNCE_MS, move || {
                handle.save_now();
            });
            handle.save_timer.set_value(Some(timeout));
        });
        let handle = *self;
        let _ = leptos_use::use_event_listener(
            leptos_use::use_document(),
            leptos::ev::visibilitychange,
            move |_| {
                let hidden = web_sys::window()
                    .and_then(|w| w.document())
                    .map(|d| d.hidden())
                    .unwrap_or(false);
                if hidden {
                    handle.save_now();
                }
            },
        );
    }

    /// No-op on the SSR half: there is no browser tab to debounce saves for
    /// or hide, and `open` (the only caller) never runs here in the first
    /// place.
    #[cfg(not(feature = "hydrate"))]
    fn install_persistence(&self) {}
}
