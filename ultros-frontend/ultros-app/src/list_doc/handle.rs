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
use ultros_list_doc::{
    DocError, ImportReport, ListDocument, ListUndo, MetaSnapshot, RowSnapshot, Subscription,
};

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
    /// Set by `purge`. Once true, this handle is inert for persistence: no
    /// more debounced or immediate saves happen for this user+list, since
    /// the local copy has been explicitly discarded. The page constructs a
    /// fresh handle if it re-opens the list.
    purged: RwSignal<bool>,
    /// Set by [`Self::close`]. A closed handle is detached from its
    /// document's change subscriptions, so an edit applied to it would
    /// mutate a document nothing is listening to: no revision bump, no
    /// outbox entry, no save. Callers must therefore refuse to edit through
    /// a closed handle rather than silently dropping the edit — see
    /// [`Self::is_closed_or_disposed`].
    closed: RwSignal<bool>,
}

impl ListDocHandle {
    /// Load the browser's snapshot for this user+list, or start empty.
    /// Loro's default random peer id is used; nothing stores or reuses peer
    /// ids. Browser-only — see the module doc comment.
    pub fn open(user_id: i64, list_id: i32) -> Self {
        let loaded = store::load(&BrowserStorage, user_id, list_id);
        let doc = loaded
            .as_ref()
            .and_then(|l| match ListDocument::from_snapshot(&l.snapshot) {
                Ok(doc) => Some(doc),
                Err(_) => {
                    tracing::warn!(user_id, list_id, "corrupt list snapshot; purging");
                    store::purge(&BrowserStorage, user_id, list_id);
                    None
                }
            });
        let doc = doc.unwrap_or_default();
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
            purged: RwSignal::new(false),
            closed: RwSignal::new(false),
        };
        handle.install_persistence();
        handle
    }

    /// Whether this handle's stored values are gone because their owner was
    /// disposed. A `ListDocHandle` is `Copy` and outlives its reactive
    /// nodes: socket callbacks, save timers and the page's own cleanup can
    /// all still hold one after the page was torn down. Reading a disposed
    /// `StoredValue`/signal panics — which on wasm is an `unreachable` that
    /// takes the whole module down — so every accessor below checks first
    /// and behaves as though the handle were simply closed.
    fn is_disposed(&self) -> bool {
        self.doc.try_with_value(|_| ()).is_none()
    }

    /// Whether this handle can still accept an edit. False once the page has
    /// [`closed`](Self::close) it (its document subscriptions are gone, so a
    /// mutation would notify nobody) and false once its nodes are disposed.
    /// The page checks this before routing a user action into
    /// [`Self::apply`], so a closed document reports an error instead of
    /// swallowing the edit.
    pub fn is_closed_or_disposed(&self) -> bool {
        self.is_disposed() || self.closed.try_get_untracked().unwrap_or(true)
    }

    /// Runs `f` against the document, or `None` once this handle is closed
    /// (see [`Self::is_closed`]).
    pub fn with_doc<R>(&self, f: impl FnOnce(&ListDocument) -> R) -> Option<R> {
        match self.doc.try_with_value(f) {
            Some(value) => Some(value),
            None => {
                log::debug!(
                    "list {}: document accessed after its owner was disposed",
                    self.list_id
                );
                None
            }
        }
    }

    pub fn rows(&self) -> Vec<RowSnapshot> {
        self.with_doc(|doc| doc.rows()).unwrap_or_default()
    }

    pub fn meta(&self) -> MetaSnapshot {
        self.with_doc(|doc| doc.meta())
            .unwrap_or_else(|| MetaSnapshot {
                name: String::new(),
                scope: None,
            })
    }

    /// The document's version vector, for callers that may outlive the
    /// handle's owner — which, now that `routes/list_view_sync.rs` is the
    /// only call site, is all of them. The socket's reconnect replay
    /// rebuilds a list-doc subscribe message from a factory that can still
    /// be in the `subscription_messages` map after the page (and this
    /// handle's `StoredValue`s) were disposed. Reading a disposed
    /// `StoredValue` panics, so a late replay would abort the wasm module;
    /// an empty version instead just asks the server for a full snapshot,
    /// which the (already dead) subscription then ignores.
    pub fn try_version(&self) -> Vec<u8> {
        self.doc
            .try_with_value(|doc| doc.version())
            .unwrap_or_default()
    }

    /// One user action, one undo step. The document commits inside, which
    /// bumps `revision` and pushes the update onto `outbox`.
    pub fn apply(&self, edit: Edit) -> Result<(), DocError> {
        // A closed (or disposed) handle has no live subscriptions and no
        // page to show a result: mutating it would be invisible. The page
        // guards this with `is_closed_or_disposed` and reports "document is
        // closed"; this is the belt-and-braces half.
        if self.is_closed_or_disposed() {
            log::debug!("list {}: edit dropped, handle closed", self.list_id);
            return Ok(());
        }
        self.with_doc(|doc| {
            let mut result = Ok(());
            if self
                .undo
                .try_update_value(|undo| result = adapter::apply(doc, undo, edit))
                .is_none()
            {
                log::debug!("list {}: edit dropped, handle closed", self.list_id);
            }
            result
        })
        // A closed handle has no document to edit and no page to show the
        // result: report success rather than panicking on a disposed node.
        .unwrap_or(Ok(()))
    }

    /// Surfaces `ImportReport::pending`: a `true` value means the imported
    /// bytes were parked because they depend on history this document
    /// hasn't seen yet, so the document was NOT brought up to date by this
    /// call. Callers must not treat a pending import as having converged
    /// the document (F1) — see `list_doc::sync`'s handshake arm.
    pub fn import(&self, bytes: &[u8]) -> Result<ImportReport, DocError> {
        // `pending: false` is the neutral answer for a closed handle: the
        // caller must not schedule a resync for a document that is gone.
        if self.is_closed_or_disposed() {
            return Ok(ImportReport { pending: false });
        }
        self.with_doc(|doc| doc.import(bytes))
            .unwrap_or(Ok(ImportReport { pending: false }))
    }

    pub fn export_since(&self, version: &[u8]) -> Result<Vec<u8>, DocError> {
        self.with_doc(|doc| doc.export_since(version))
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    pub fn is_ahead_of(&self, version: &[u8]) -> bool {
        self.with_doc(|doc| doc.is_ahead_of(version))
            .unwrap_or(false)
    }

    pub fn undo(&self) -> bool {
        let mut done = false;
        let _ = self
            .undo
            .try_update_value(|undo| done = undo.undo().unwrap_or(false));
        done
    }

    pub fn redo(&self) -> bool {
        let mut done = false;
        let _ = self
            .undo
            .try_update_value(|undo| done = undo.redo().unwrap_or(false));
        done
    }

    /// Whether `undo` would change anything. Not reactive on its own: read it
    /// under `revision.track()` so it follows the document.
    pub fn can_undo(&self) -> bool {
        self.undo
            .try_with_value(|undo| undo.can_undo())
            .unwrap_or(false)
    }

    /// Whether `redo` would change anything. See [`Self::can_undo`].
    pub fn can_redo(&self) -> bool {
        self.undo
            .try_with_value(|undo| undo.can_redo())
            .unwrap_or(false)
    }

    /// Only a real change notifies: the sync loop reports "live" on every
    /// relayed update, and re-notifying an unchanged status would re-render
    /// everything that shows it.
    pub fn set_status(&self, status: &str) {
        if self
            .status
            .try_with_untracked(|s| s != status)
            .unwrap_or(false)
        {
            let _ = self.status.try_set(status.to_string());
        }
    }

    pub fn remember_permission(&self, permission: i16) {
        if self.is_closed_or_disposed() {
            return;
        }
        let _ = self.permission.try_set(permission);
        let _ = store::remember_permission(
            &BrowserStorage,
            self.user_id,
            self.list_id,
            permission,
            store::now_ms(),
        );
    }

    pub fn save_now(&self) {
        // A disposed `purged` signal reads as `None`; treat that as "nothing
        // worth saving" rather than panicking (see `is_closed`).
        if self.purged.try_get_untracked().unwrap_or(true) {
            return;
        }
        let Some(snapshot) = self.with_doc(|doc| doc.export_snapshot()) else {
            return;
        };
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
    ///
    /// Marks the handle `purged`, which makes it inert for persistence:
    /// `save_now` and the debounced-save `Effect` both return early from
    /// this point on, so the revision bump below does not resurrect the
    /// snapshot or index entry that were just removed. The handle stays
    /// inert until it is dropped; the page opens a fresh `ListDocHandle` if
    /// it re-opens the list.
    pub fn purge(&self) {
        // The browser copy still has to go even for a closed handle, but the
        // in-memory swap below would touch disposed nodes, so stop after the
        // storage half.
        if self.is_disposed() {
            store::purge(&BrowserStorage, self.user_id, self.list_id);
            return;
        }
        self.purged.set(true);
        store::purge(&BrowserStorage, self.user_id, self.list_id);
        // Clear the outbox before swapping documents so a subscription
        // firing mid-swap cannot re-add anything from the discarded
        // document; commits from it must never reach the socket.
        self.outbox.set(Vec::new());
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

    /// Replace the document with `snapshot` (the server's truth) and re-apply
    /// the local rows on top as NEW operations, so edits the server could not
    /// accept (history it compacted, or a forbidden meta change) are
    /// re-expressed against the server's history. `keep_local_meta = false`
    /// drops a local name/scope change. Returns whether anything local had to
    /// be re-applied.
    ///
    /// This is the escape from the resync loop: after a rebase the local
    /// document descends only from history the server has, so the next
    /// `export_since(server_version)` is something it can actually merge.
    /// Undo history restarts — the operations the stack pointed at no longer
    /// exist — and the local rows survive as ordinary new edits.
    pub fn rebase_onto_snapshot(
        &self,
        snapshot: &[u8],
        keep_local_meta: bool,
    ) -> Result<bool, DocError> {
        if self.is_closed_or_disposed() {
            return Ok(false);
        }
        let local_rows = self.rows();
        let local_meta = self.meta();
        let fresh = ListDocument::from_snapshot(snapshot)?;
        let undo = ListUndo::new(&fresh);
        let revision = self.revision;
        let outbox = self.outbox;
        let on_change = fresh.on_change(move || revision.update(|r| *r += 1));
        let on_local = fresh.on_local_update(move |bytes| {
            let bytes = bytes.to_vec();
            outbox.update(|queue| queue.push(bytes));
        });
        // Cleared before the swap, like `purge`: whatever the abandoned
        // document had queued depends on history the server rejected, and
        // the re-applied mutations below refill the outbox with operations
        // it can accept.
        self.outbox.set(Vec::new());
        self.doc.set_value(fresh);
        self.undo.set_value(undo);
        self.subscriptions.set_value(vec![on_change, on_local]);

        let Some(reapplied) = self.with_doc(|doc| ultros_list_doc::rebase_rows(doc, &local_rows))
        else {
            return Ok(false);
        };
        let mut reapplied = reapplied?;
        if keep_local_meta {
            let applied_meta = self.with_doc(|doc| -> Result<bool, DocError> {
                let server_meta = doc.meta();
                let mut applied = false;
                if server_meta.name != local_meta.name {
                    doc.rename(&local_meta.name)?;
                    applied = true;
                }
                if let Some(scope) = local_meta.scope
                    && server_meta.scope != Some(scope)
                {
                    doc.set_scope(scope)?;
                    applied = true;
                }
                Ok(applied)
            });
            reapplied |= applied_meta.unwrap_or(Ok(false))?;
        }
        self.revision.update(|r| *r += 1);
        self.save_now();
        Ok(reapplied)
    }

    /// Stop this handle from doing any more background work: flushes any
    /// pending save, cancels the timer, and drops the document
    /// subscriptions. Call when a page navigates away from this list. A
    /// purged handle has nothing worth saving, so the flush is skipped.
    pub fn close(&self) {
        // Idempotent, and safe to call after the owner is gone: the page
        // closes the handle from inside the lifecycle Effect's cleanup and
        // again from its own `on_cleanup`.
        if self.is_closed_or_disposed() {
            return;
        }
        if !self.purged.try_get_untracked().unwrap_or(true) {
            self.save_now();
        }
        let _ = self.save_timer.try_update_value(|timer| *timer = None);
        let _ = self.subscriptions.try_update_value(|subs| subs.clear());
        self.closed.set(true);
    }

    /// Release every reactive node this handle owns. Call only on a handle
    /// the page has already [`closed`](Self::close) and replaced — navigating
    /// between lists would otherwise leave each superseded document, its undo
    /// stack and its signals in the owner's arena for the lifetime of the
    /// page. Every accessor above goes through `try_*`, so the copies of this
    /// handle still held by a socket callback or a queued timer keep behaving
    /// as though it were closed instead of panicking.
    pub fn dispose(self) {
        self.doc.dispose();
        self.undo.dispose();
        self.subscriptions.dispose();
        self.save_timer.dispose();
        self.revision.dispose();
        self.outbox.dispose();
        self.status.dispose();
        self.permission.dispose();
        self.purged.dispose();
        self.closed.dispose();
    }

    /// Save half a second after the last change, and immediately when the
    /// tab is hidden. Client only: the server has no storage to write.
    #[cfg(feature = "hydrate")]
    fn install_persistence(&self) {
        let handle = *self;
        Effect::new(move |_| {
            // Every access is `try_*`: this Effect belongs to the page's
            // owner, so it outlives a handle the page disposed after
            // superseding it (`ListDocHandle::dispose`), and reading a
            // disposed signal on wasm aborts the module.
            if handle.revision.try_get().is_none() {
                return;
            }
            // Dropping the previous timeout cancels it.
            let _ = handle.save_timer.try_update_value(|timer| *timer = None);
            if handle.purged.try_get_untracked().unwrap_or(true) {
                return;
            }
            let timeout = gloo_timers::callback::Timeout::new(SAVE_DEBOUNCE_MS, move || {
                handle.save_now();
            });
            let _ = handle
                .save_timer
                .try_update_value(move |timer| *timer = Some(timeout));
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
