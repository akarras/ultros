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

use ultros_list_doc::recovery::{self, RecoveryError};

use crate::list_doc::adapter::{self, Edit};
use crate::list_doc::store::{self, BrowserStorage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveState {
    Pending,
    Saved,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryState {
    None,
    Recovered {
        dropped_meta: bool,
    },
    Review,
    /// The original cache cannot be interpreted safely by this app version.
    Incompatible,
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen(module = "/../../ultros/static/account-list-store.mjs")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = accountSaveLocked)]
    fn save_locked(
        user: &str,
        list: i32,
        token: f64,
        callback: &js_sys::Function,
    ) -> js_sys::Promise;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = accountRemember)]
    fn remember(
        user: &str,
        list: i32,
        snapshot: &js_sys::Uint8Array,
        replacement_source: &wasm_bindgen::JsValue,
        content_ready: bool,
    ) -> f64;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = accountRecovery)]
    fn recovery(user: &str, list: i32) -> wasm_bindgen::JsValue;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = accountRecoverySource)]
    fn recovery_source(user: &str, list: i32) -> wasm_bindgen::JsValue;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = accountRecoveryReady)]
    fn recovery_ready(user: &str, list: i32) -> wasm_bindgen::JsValue;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = accountForget)]
    fn forget(user: &str, list: i32);
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = accountPending)]
    fn pending(user: &str, list: i32, token: f64) -> bool;
    #[wasm_bindgen::prelude::wasm_bindgen(catch, js_name = accountDownload)]
    fn download(name: &str, snapshot: &js_sys::Uint8Array) -> Result<(), wasm_bindgen::JsValue>;
}

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
    /// Permission can arrive over REST before the document handshake. An
    /// untouched receiver is not a known empty list.
    content_ready: RwSignal<bool>,
    pub save_state: RwSignal<SaveState>,
    pub recovery_state: RwSignal<RecoveryState>,
    pub recovery_retry: RwSignal<u64>,
    pub recovery_saved: RwSignal<u64>,
    /// Rebased operation IDs must be durable before the server accepts them.
    /// Otherwise an old disk copy could replay the same intent under new IDs.
    pub recovery_persisting: RwSignal<bool>,
    recovery_snapshot: StoredValue<Option<Vec<u8>>, LocalStorage>,
    incompatible_snapshot: StoredValue<Option<Vec<u8>>, LocalStorage>,
    generation: StoredValue<String, LocalStorage>,
    replacement_source: StoredValue<Option<Vec<u8>>, LocalStorage>,
    revoked: StoredValue<std::rc::Rc<std::cell::Cell<bool>>, LocalStorage>,
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
        Self::open_loaded(user_id, list_id, loaded)
    }

    fn open_loaded(user_id: i64, list_id: i32, loaded: Option<store::Loaded>) -> Self {
        let mut incompatible_snapshot = None;
        let doc = loaded
            .as_ref()
            .and_then(|l| match ListDocument::from_snapshot(&l.snapshot) {
                Ok(doc) => Some(doc),
                Err(error) => {
                    tracing::warn!(user_id, list_id, %error, "list snapshot preserved for recovery");
                    incompatible_snapshot = Some(l.snapshot.clone());
                    None
                }
            });
        let doc = doc.unwrap_or_else(ListDocument::empty_peer);
        let content_ready = loaded
            .as_ref()
            .and_then(|loaded| loaded.content_ready)
            .unwrap_or_else(|| store::causal_content_ready(&doc));
        #[cfg(not(feature = "hydrate"))]
        let replacement_source: Option<Vec<u8>> = None;
        #[cfg(feature = "hydrate")]
        let (doc, replacement_source, content_ready) = {
            // A failed replacement survives SPA navigation with its source
            // fence. Never merge it with the abandoned history on disk or send
            // its new operation IDs before completing the guarded save.
            let pending = recovery(&user_id.to_string(), list_id);
            let source = recovery_source(&user_id.to_string(), list_id);
            let pending_ready = recovery_ready(&user_id.to_string(), list_id).as_bool();
            if incompatible_snapshot.is_none() && !pending.is_null() {
                let bytes = js_sys::Uint8Array::new(&pending).to_vec();
                match ListDocument::from_snapshot(&bytes) {
                    Err(_) => {
                        incompatible_snapshot = Some(bytes);
                        (doc, None, false)
                    }
                    Ok(replacement) if !source.is_null() => {
                        let ready = pending_ready
                            .unwrap_or_else(|| store::causal_content_ready(&replacement));
                        (
                            replacement,
                            Some(js_sys::Uint8Array::new(&source).to_vec()),
                            ready,
                        )
                    }
                    Ok(replacement) => {
                        let ready = pending_ready
                            .unwrap_or_else(|| store::causal_content_ready(&replacement));
                        let (doc, ready) = match doc.import(&bytes) {
                            Ok(report) if !report.pending => (doc, content_ready || ready),
                            _ => (replacement, ready),
                        };
                        (doc, None, ready)
                    }
                }
            } else {
                (doc, None, content_ready)
            }
        };
        let recovery_persisting = replacement_source.is_some();
        let incompatible = incompatible_snapshot.is_some();
        let content_ready = !incompatible && content_ready;
        let permission = if incompatible {
            0
        } else {
            loaded.map(|l| l.permission).unwrap_or(0)
        };
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
            status: RwSignal::new(
                if incompatible {
                    "offline"
                } else {
                    "connecting"
                }
                .to_string(),
            ),
            permission: RwSignal::new(permission),
            content_ready: RwSignal::new(content_ready),
            save_state: RwSignal::new(if incompatible {
                SaveState::Failed
            } else {
                SaveState::Pending
            }),
            recovery_state: RwSignal::new(if incompatible {
                RecoveryState::Incompatible
            } else if recovery_persisting {
                RecoveryState::Recovered {
                    dropped_meta: false,
                }
            } else {
                RecoveryState::None
            }),
            recovery_retry: RwSignal::new(0),
            recovery_saved: RwSignal::new(0),
            recovery_persisting: RwSignal::new(recovery_persisting),
            recovery_snapshot: StoredValue::new_local(None),
            incompatible_snapshot: StoredValue::new_local(incompatible_snapshot),
            generation: StoredValue::new_local(store::generation(
                &BrowserStorage,
                user_id,
                list_id,
            )),
            replacement_source: StoredValue::new_local(replacement_source),
            revoked: StoredValue::new_local(std::rc::Rc::new(std::cell::Cell::new(false))),
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

    /// Reactive readiness, independent of whether REST has granted access.
    pub fn has_readable_content(&self) -> bool {
        !self.is_closed_or_disposed()
            && self.content_ready.try_get().unwrap_or(false)
            && !self.incompatible()
    }

    /// Only the completed document handshake (including UpToDate) establishes
    /// an initially empty receiver as authoritative contents.
    pub fn mark_content_ready(&self) {
        if !self.is_closed_or_disposed() && !self.incompatible() {
            let _ = self.content_ready.try_set(true);
        }
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
        if self.incompatible() {
            return Err(DocError::InvalidStructure(
                "saved copy; export recovery and update Ultros".into(),
            ));
        }
        // A closed (or disposed) handle has no live subscriptions and no
        // page to show a result: mutating it would be invisible. The page
        // guards this with `is_closed_or_disposed` and reports "document is
        // closed"; this is the belt-and-braces half.
        if self.is_closed_or_disposed() {
            log::debug!("list {}: edit dropped, handle closed", self.list_id);
            return Ok(());
        }
        if !self.has_readable_content() {
            return Err(DocError::InvalidStructure(
                "list contents are still loading".into(),
            ));
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

    /// Surfaces `ImportReport::pending`: a `true` value means the incoming
    /// bytes were not imported because they depend on history this document
    /// hasn't seen yet, so the document was NOT brought up to date by this
    /// call. Callers must not treat a pending import as having converged
    /// the document (F1) — see `list_doc::sync`'s handshake arm.
    pub fn import(&self, bytes: &[u8]) -> Result<ImportReport, DocError> {
        if self.incompatible() {
            return Err(DocError::InvalidStructure(
                "saved copy; export recovery and update Ultros".into(),
            ));
        }
        // `pending: false` is the neutral answer for a closed handle: the
        // caller must not schedule a resync for a document that is gone.
        if self.is_closed_or_disposed() {
            return Ok(ImportReport { pending: false });
        }
        let result = self
            .with_doc(|doc| doc.import(bytes))
            .unwrap_or(Ok(ImportReport { pending: false }));
        if let Err(error) = &result {
            self.pause_incompatible_import(error);
        }
        result
    }

    /// Probe snapshot imports away from the live document. A parked/partial
    /// import must not alter the history or projection used to recover intent.
    pub fn import_server_snapshot(&self, bytes: &[u8]) -> Result<ImportReport, DocError> {
        let probe = self.with_doc(|doc| -> Result<ImportReport, DocError> {
            let server = ListDocument::from_snapshot(bytes)?;
            if !server.can_export_since(&doc.version()) {
                // Recover before importing: even an apparently successful
                // shallow merge could resolve a same-field conflict and erase
                // the local value the player needs to review.
                return Ok(ImportReport { pending: true });
            }
            let copy = ListDocument::from_snapshot(&doc.export_snapshot()?)?;
            copy.import(bytes)
        });
        match probe {
            Some(Ok(ImportReport { pending: true }) | Err(DocError::OutdatedDependency)) => {
                Ok(ImportReport { pending: true })
            }
            Some(Err(error)) => {
                self.pause_incompatible_import(&error);
                Err(error)
            }
            _ => self.import(bytes),
        }
    }

    fn pause_incompatible_import(&self, error: &DocError) {
        if matches!(
            error,
            DocError::UnsupportedSchema(_)
                | DocError::InvalidStructure(_)
                | DocError::IncompleteSnapshot
        ) && !self.is_closed_or_disposed()
            && !self.incompatible()
        {
            // Keep the valid local state (including unsent work), not the
            // rejected peer payload. The durable copy is never overwritten.
            let original = self.with_doc(|doc| doc.export_snapshot().ok()).flatten();
            #[cfg(feature = "hydrate")]
            if let Some(snapshot) = &original {
                // A peer error may arrive before the local save debounce.
                // Keep those unsent edits across SPA navigation as well.
                self.remember_snapshot(snapshot);
            }
            self.incompatible_snapshot.set_value(original);
            self.recovery_state.set(RecoveryState::Incompatible);
            self.permission.set(0);
            self.set_status("offline");
        }
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
        if !self.has_readable_content() {
            return false;
        }
        let mut done = false;
        let _ = self
            .undo
            .try_update_value(|undo| done = undo.undo().unwrap_or(false));
        done
    }

    pub fn redo(&self) -> bool {
        if !self.has_readable_content() {
            return false;
        }
        let mut done = false;
        let _ = self
            .undo
            .try_update_value(|undo| done = undo.redo().unwrap_or(false));
        done
    }

    /// Reactive: tracks the document revision, which every local edit, undo,
    /// redo, import and rebase bumps, so a toolbar can derive its disabled
    /// state from this (#1430). A closed or disposed handle has nothing to
    /// undo.
    pub fn can_undo(&self) -> bool {
        let _ = self.revision.try_get();
        let _ = self.recovery_state.try_get();
        if !self.has_readable_content() {
            return false;
        }
        self.undo
            .try_with_value(|undo| undo.can_undo())
            .unwrap_or(false)
    }

    pub fn can_redo(&self) -> bool {
        let _ = self.revision.try_get();
        let _ = self.recovery_state.try_get();
        if !self.has_readable_content() {
            return false;
        }
        self.undo
            .try_with_value(|undo| undo.can_redo())
            .unwrap_or(false)
    }

    pub fn can_undo_purchase(&self) -> bool {
        let _ = self.revision.try_get();
        let _ = self.recovery_state.try_get();
        self.has_readable_content()
            && self
                .undo
                .try_with_value(|undo| undo.can_undo_purchase())
                .unwrap_or(false)
    }

    /// Only a real change notifies: the sync loop reports "live" on every
    /// relayed update, and re-notifying an unchanged status would re-render
    /// everything that shows it.
    pub fn set_status(&self, status: &str) {
        // Transport reconnects cannot resume a document awaiting compatibility
        // recovery. Keep its badge consistent with the paused outbox.
        let status = if self.incompatible() {
            "offline"
        } else {
            status
        };
        if self
            .status
            .try_with_untracked(|s| s != status)
            .unwrap_or(false)
        {
            let _ = self.status.try_set(status.to_string());
        }
    }

    pub fn remember_permission(&self, permission: i16) {
        if self.is_closed_or_disposed() || self.incompatible() {
            return;
        }
        let _ = self.permission.try_set(permission);
        self.update_index(Some(permission));
    }

    /// All account-index writers use the same lock. Snapshot purge remains
    /// synchronous; its delayed index cleanup cannot remove a reopened copy.
    fn update_index(&self, permission: Option<i16>) {
        #[cfg(feature = "hydrate")]
        {
            use wasm_bindgen::{JsCast, prelude::*};
            let user = self.user_id;
            let list = self.list_id;
            let Ok(generation) = store::checked_generation(&BrowserStorage, user, list) else {
                return;
            };
            leptos::task::spawn_local(async move {
                let callback = Closure::<dyn FnMut() -> JsValue>::new(move || {
                    if store::checked_generation(&BrowserStorage, user, list).as_deref()
                        == Ok(generation.as_str())
                    {
                        match permission {
                            Some(permission) => {
                                store::remember_permission(
                                    &BrowserStorage,
                                    user,
                                    list,
                                    permission,
                                    store::now_ms(),
                                );
                            }
                            None => {
                                store::purge_index(&BrowserStorage, user, list);
                            }
                        }
                    }
                    // Index maintenance never acknowledges a pending snapshot.
                    JsValue::FALSE
                });
                let _ = wasm_bindgen_futures::JsFuture::from(save_locked(
                    &user.to_string(),
                    list,
                    0.0,
                    callback.as_ref().unchecked_ref(),
                ))
                .await;
            });
        }
        #[cfg(not(feature = "hydrate"))]
        {
            match permission {
                Some(permission) => {
                    store::remember_permission(
                        &BrowserStorage,
                        self.user_id,
                        self.list_id,
                        permission,
                        store::now_ms(),
                    );
                }
                None => {
                    store::purge_index(&BrowserStorage, self.user_id, self.list_id);
                }
            }
        }
    }

    // Keep export failure handling before the persistence boundary. The caller
    // receives bytes only after export succeeds; retry must not claim durability.
    fn prepare_save(
        &self,
        export: impl FnOnce(&ListDocument) -> Result<Vec<u8>, DocError>,
    ) -> Option<Vec<u8>> {
        if self.purged.try_get_untracked().unwrap_or(true) || self.incompatible() {
            return None;
        }
        if !self.content_ready.try_get_untracked().unwrap_or(false)
            && self.with_doc(|doc| doc.inner().oplog_vv().iter().next().is_none()) == Some(true)
        {
            // Do not turn a permission-only first open into a saved empty
            // document. Real local intent remains recoverable before sync.
            return None;
        }
        let snapshot = self.with_doc(export)?;
        let Ok(snapshot) = snapshot else {
            let _ = self.save_state.try_set(SaveState::Failed);
            return None;
        };
        let _ = self.save_state.try_set(SaveState::Pending);
        Some(snapshot)
    }

    #[cfg(feature = "hydrate")]
    fn remember_snapshot(&self, snapshot: &[u8]) -> f64 {
        let source = self
            .replacement_source
            .get_value()
            .map(|bytes| wasm_bindgen::JsValue::from(js_sys::Uint8Array::from(bytes.as_slice())))
            .unwrap_or(wasm_bindgen::JsValue::NULL);
        remember(
            &self.user_id.to_string(),
            self.list_id,
            &js_sys::Uint8Array::from(snapshot),
            &source,
            self.content_ready.get_untracked(),
        )
    }

    pub fn save_now(&self) {
        let Some(snapshot) = self.prepare_save(ListDocument::export_snapshot) else {
            return;
        };
        #[cfg(feature = "hydrate")]
        {
            use wasm_bindgen::{JsCast, prelude::*};
            let handle = *self;
            let permission = self.permission.get_untracked();
            let content_ready = self.content_ready.get_untracked();
            let generation = self.generation.get_value();
            let replacement_source = self.replacement_source.get_value();
            let revoked = self.revoked.get_value();
            let token = self.remember_snapshot(&snapshot);
            // Snapshot and revocation token outlive the page. Navigation can
            // dispose the reactive handle while this task waits for another tab.
            leptos::task::spawn_local(async move {
                let callback = Closure::<dyn FnMut() -> JsValue>::new(move || {
                    if revoked.get() {
                        return JsValue::FALSE;
                    }
                    let result = if let Some(source_version) = replacement_source.as_deref() {
                        let (next, result) = store::replace_save_with_readiness(
                            &BrowserStorage,
                            handle.user_id,
                            handle.list_id,
                            (&snapshot, content_ready),
                            permission,
                            &generation,
                            source_version,
                        );
                        if !handle.is_closed_or_disposed()
                            && handle.generation.get_value() == generation
                        {
                            handle.generation.set_value(next);
                            if result.is_ok() {
                                handle.replacement_source.set_value(None);
                            }
                        }
                        result
                    } else {
                        store::merge_save_with_readiness(
                            &BrowserStorage,
                            handle.user_id,
                            handle.list_id,
                            (&snapshot, content_ready),
                            permission,
                            &generation,
                        )
                    };
                    match result {
                        Ok(merged) => {
                            // A pre-rebase save can finish while the replacement
                            // waits for the lock. Its old operations must never
                            // be imported into the new document a second time.
                            if !(handle.is_closed_or_disposed()
                                || handle.recovery_persisting.get_untracked()
                                    && replacement_source.is_none())
                            {
                                let before = handle.try_version();
                                match handle.import(&merged) {
                                    Ok(report) if !report.pending => {
                                        if handle.is_ahead_of(&before) {
                                            // Imported peer operations are not undoable local edits,
                                            // but still need relaying when this tab is connected.
                                            handle
                                                .outbox
                                                .update(|queue| queue.push(merged.clone()));
                                        }
                                        let saved = ListDocument::from_snapshot(&merged)
                                            .is_ok_and(|doc| !handle.is_ahead_of(&doc.version()));
                                        if saved {
                                            handle.save_state.set(SaveState::Saved);
                                        }
                                        if replacement_source.is_some()
                                            && handle.replacement_source.get_value().is_none()
                                            && handle.recovery_persisting.get_untracked()
                                        {
                                            handle.recovery_persisting.set(false);
                                            handle.recovery_saved.update(|n| *n += 1);
                                        }
                                    }
                                    _ => {
                                        handle.save_state.set(SaveState::Failed);
                                        return JsValue::FALSE;
                                    }
                                }
                            }
                            JsValue::TRUE
                        }
                        Err(_) => JsValue::FALSE,
                    }
                });
                let result = wasm_bindgen_futures::JsFuture::from(save_locked(
                    &handle.user_id.to_string(),
                    handle.list_id,
                    token,
                    callback.as_ref().unchecked_ref(),
                ))
                .await;
                if !matches!(result, Ok(value) if value == JsValue::TRUE)
                    && pending(&handle.user_id.to_string(), handle.list_id, token)
                {
                    let _ = handle.save_state.try_set(SaveState::Failed);
                }
            });
        }
        #[cfg(not(feature = "hydrate"))]
        {
            let _ = snapshot;
            let _ = self.save_state.try_set(SaveState::Failed);
        }
    }

    pub fn download_recovery(&self) {
        #[cfg(feature = "hydrate")]
        {
            if self.is_closed_or_disposed() || self.purged.get_untracked() {
                return;
            }
            let original = self.incompatible_snapshot.get_value();
            let snapshot =
                original.or_else(|| self.with_doc(|doc| doc.export_snapshot().ok()).flatten());
            let name = self.meta().name;
            let name = if name.trim().is_empty() {
                format!("List {}", self.list_id)
            } else {
                name
            };
            if let Some(snapshot) = snapshot
                && download(&name, &js_sys::Uint8Array::from(snapshot.as_slice())).is_err()
            {
                self.save_state.set(SaveState::Failed);
            }
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
        #[cfg(feature = "hydrate")]
        forget(&self.user_id.to_string(), self.list_id);
        // The browser copy still has to go even for a closed handle, but the
        // in-memory swap below would touch disposed nodes, so stop after the
        // storage half.
        if self.is_disposed() {
            store::purge_snapshot(&BrowserStorage, self.user_id, self.list_id);
            self.update_index(None);
            return;
        }
        self.revoked.with_value(|revoked| revoked.set(true));
        self.purged.set(true);
        store::purge_snapshot(&BrowserStorage, self.user_id, self.list_id);
        self.update_index(None);
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
        self.recovery_state.set(RecoveryState::None);
        self.recovery_persisting.set(false);
        self.recovery_snapshot.set_value(None);
        self.incompatible_snapshot.set_value(None);
        self.permission.set(0);
        self.content_ready.set(false);
        self.revision.update(|r| *r += 1);
    }

    /// Reconstruct local intent from durable history before replacing anything.
    /// A failed recovery keeps the old document, undo stack and disk copy intact.
    pub fn rebase_onto_snapshot(
        &self,
        snapshot: &[u8],
        keep_local_meta: bool,
    ) -> Result<bool, RecoveryError> {
        if self.is_closed_or_disposed() || self.incompatible() {
            return Ok(false);
        }
        let recovered = self.with_doc(|doc| recovery::recover(doc, snapshot, keep_local_meta));
        match recovered {
            Some(Ok(recovered)) => {
                let changed = recovered.changed;
                self.replace_document(recovered.document);
                self.recovery_snapshot.set_value(None);
                self.recovery_state.set(RecoveryState::Recovered {
                    dropped_meta: recovered.dropped_meta,
                });
                self.save_now();
                Ok(changed)
            }
            Some(Err(error)) => {
                self.recovery_snapshot.set_value(Some(snapshot.to_vec()));
                self.recovery_state.set(RecoveryState::Review);
                self.set_status("offline");
                Err(error)
            }
            None => Ok(false),
        }
    }

    pub fn recovery_paused(&self) -> bool {
        matches!(
            self.recovery_state.try_get_untracked(),
            Some(RecoveryState::Review | RecoveryState::Incompatible)
        )
    }

    pub fn incompatible(&self) -> bool {
        self.recovery_state.try_get_untracked() == Some(RecoveryState::Incompatible)
    }

    pub fn recovery_server(&self) -> Option<ListDocument> {
        self.recovery_snapshot
            .try_with_value(|snapshot| {
                snapshot
                    .as_ref()
                    .and_then(|bytes| ListDocument::from_snapshot(bytes).ok())
            })
            .flatten()
    }

    /// The player can correct their local values and request a fresh comparison.
    pub fn retry_recovery(&self) {
        if !self.is_closed_or_disposed() && !self.incompatible() {
            self.recovery_state.set(RecoveryState::None);
            self.set_status("reconnecting");
            self.recovery_retry.update(|n| *n += 1);
        }
    }

    /// Explicit, labelled UI action after reviewing both copies. The stored
    /// replacement is still fenced against unseen changes from another tab.
    pub fn use_server_version(&self) {
        if self.is_closed_or_disposed() || !self.recovery_paused() {
            return;
        }
        if let Some(server) = self.recovery_server() {
            self.replace_document(server);
            self.recovery_snapshot.set_value(None);
            self.recovery_state.set(RecoveryState::Recovered {
                dropped_meta: false,
            });
            self.set_status("reconnecting");
            self.save_now();
        }
    }

    fn replace_document(&self, fresh: ListDocument) {
        self.recovery_persisting.set(true);
        self.replacement_source.set_value(Some(self.try_version()));
        let undo = ListUndo::new(&fresh);
        let revision = self.revision;
        let outbox = self.outbox;
        let on_change = fresh.on_change(move || revision.update(|r| *r += 1));
        let on_local = fresh.on_local_update(move |bytes| {
            outbox.update(|queue| queue.push(bytes.to_vec()));
        });
        // Old operations depend on history the server rejected. The handshake
        // sends the replacement's diff, including the recovered local intent.
        self.outbox.set(Vec::new());
        self.doc.set_value(fresh);
        self.content_ready.set(true);
        self.undo.set_value(undo);
        self.subscriptions.set_value(vec![on_change, on_local]);
        self.revision.update(|r| *r += 1);
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
        self.content_ready.dispose();
        self.save_state.dispose();
        self.recovery_state.dispose();
        self.recovery_retry.dispose();
        self.recovery_saved.dispose();
        self.recovery_persisting.dispose();
        self.recovery_snapshot.dispose();
        self.incompatible_snapshot.dispose();
        self.generation.dispose();
        self.replacement_source.dispose();
        self.revoked.dispose();
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
            if handle.purged.try_get_untracked().unwrap_or(true) || handle.incompatible() {
                return;
            }
            if !handle.content_ready.try_get().unwrap_or(false)
                && handle.with_doc(|doc| doc.inner().oplog_vv().iter().next().is_none())
                    == Some(true)
            {
                return;
            }
            let _ = handle.save_state.try_set(SaveState::Pending);
            // Keep the latest edit recoverable during the debounce window too.
            if let Some(Ok(snapshot)) = handle.with_doc(|doc| doc.export_snapshot()) {
                handle.remember_snapshot(&snapshot);
            }
            let timeout = gloo_timers::callback::Timeout::new(SAVE_DEBOUNCE_MS, move || {
                handle.save_now();
            });
            let _ = handle
                .save_timer
                .try_update_value(move |timer| *timer = Some(timeout));
        });
        let handle = *self;
        let key = format!("ultros.listdoc.v1.{}.{}", self.user_id, self.list_id);
        let _ = leptos_use::use_event_listener(
            leptos_use::use_window(),
            leptos::ev::storage,
            move |event| {
                if !handle.is_closed_or_disposed() && event.key().as_deref() == Some(key.as_str()) {
                    if event.new_value().is_none() {
                        handle.purge();
                    } else {
                        handle.save_now();
                    }
                }
            },
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_permission_does_not_make_an_empty_receiver_readable() {
        Owner::new().with(|| {
            for cached in [
                None,
                Some(store::Loaded {
                    content_ready: None,
                    snapshot: ListDocument::empty_peer().export_snapshot().unwrap(),
                    permission: 3,
                }),
            ] {
                let handle = ListDocHandle::open_loaded(1, 1, cached);
                handle.remember_permission(3);
                assert_eq!(handle.permission.get_untracked(), 3);
                assert!(!handle.has_readable_content());
                assert!(
                    handle
                        .prepare_save(|_| panic!("initial receiver must not be saved"))
                        .is_none()
                );
                // UpToDate is authoritative too: a server may explicitly
                // confirm an operation-free legacy empty document.
                handle.mark_content_ready();
                assert!(handle.has_readable_content());
                assert!(handle.prepare_save(ListDocument::export_snapshot).is_some());
                handle.purge();
                assert!(!handle.has_readable_content());
            }
        });
    }

    #[test]
    fn compacted_cached_empty_and_nonempty_documents_are_readable_offline() {
        Owner::new().with(|| {
            for with_row in [false, true] {
                let doc = ListDocument::new();
                // No name heuristic: an empty name is still valid metadata.
                if with_row {
                    doc.add_row(ultros_list_doc::RowKey::new(5056, None), 3, None)
                        .unwrap();
                }
                let handle = ListDocHandle::open_loaded(
                    1,
                    1,
                    Some(store::Loaded {
                        content_ready: None,
                        snapshot: doc.export_shallow().unwrap(),
                        permission: 3,
                    }),
                );
                handle.set_status("offline");
                assert!(handle.has_readable_content());
                assert_eq!(handle.rows().is_empty(), !with_row);
                handle.close();
                assert!(!handle.has_readable_content());
            }
        });
    }

    #[test]
    fn content_readiness_requires_handshake_and_does_not_discard_local_intent() {
        Owner::new().with(|| {
            let handle = ListDocHandle::open_loaded(1, 1, None);
            let doc = ListDocument::new();
            doc.add_row(ultros_list_doc::RowKey::new(5056, None), 3, None)
                .unwrap();
            handle.import(&doc.export_snapshot().unwrap()).unwrap();
            assert!(
                !handle.has_readable_content(),
                "ordinary imports are not handshake completion"
            );
            // Recovery bytes with real operations remain preservable while
            // contents wait for the authoritative handshake.
            assert!(handle.prepare_save(ListDocument::export_snapshot).is_some());
            handle.mark_content_ready();
            assert!(handle.has_readable_content());
            handle.close();
        });
    }

    #[test]
    fn explicit_cache_provenance_overrides_history_and_permission() {
        Owner::new().with(|| {
            for ready in [false, true] {
                let doc = if ready {
                    ListDocument::empty_peer()
                } else {
                    ListDocument::new()
                };
                let handle = ListDocHandle::open_loaded(
                    1,
                    1,
                    Some(store::Loaded {
                        snapshot: doc.export_snapshot().unwrap(),
                        permission: 3,
                        content_ready: Some(ready),
                    }),
                );
                assert_eq!(handle.has_readable_content(), ready);
                if !ready {
                    assert!(handle.apply(Edit::Remove(123)).is_err());
                    assert!(!handle.undo());
                    assert!(!handle.redo());
                    assert!(!handle.can_undo());
                }
                handle.close();
            }
        });
    }

    #[test]
    fn incompatible_cached_document_stays_exportable_and_cannot_be_overwritten() {
        Owner::new().with(|| {
            let future = ListDocument::new();
            future
                .inner()
                .get_map("meta")
                .insert("schema", 999_i64)
                .unwrap();
            future.commit();
            for original in [
                future.export_snapshot().unwrap(),
                b"damaged Loro bytes".to_vec(),
            ] {
                let handle = ListDocHandle::open_loaded(
                    1,
                    1,
                    Some(store::Loaded {
                        content_ready: None,
                        snapshot: original.clone(),
                        permission: 3,
                    }),
                );
                assert!(handle.incompatible());
                assert!(handle.recovery_paused());
                assert_eq!(handle.status.get_untracked(), "offline");
                handle.set_status("connecting");
                handle.set_status("live");
                assert_eq!(handle.status.get_untracked(), "offline");
                assert_eq!(handle.permission.get_untracked(), 0);
                assert_eq!(
                    handle.incompatible_snapshot.get_value(),
                    Some(original.clone())
                );
                assert!(
                    handle
                        .prepare_save(|_| panic!("must not export replacement"))
                        .is_none()
                );
                assert!(
                    handle
                        .import(&ListDocument::new().export_snapshot().unwrap())
                        .is_err()
                );
                handle.retry_recovery();
                handle.remember_permission(3);
                assert!(handle.incompatible());
                assert_eq!(handle.permission.get_untracked(), 0);
                assert_eq!(handle.incompatible_snapshot.get_value(), Some(original));
                handle.close();
            }
        });
    }

    fn recovery_peers() -> (ListDocHandle, ListDocument, ultros_list_doc::RowKey) {
        let key = ultros_list_doc::RowKey::new(1, None);
        let server = ListDocument::new();
        server.rename("Shared").unwrap();
        server.add_row(key, 10, None).unwrap();
        let handle = ListDocHandle::open(1, 1);
        handle.import(&server.export_snapshot().unwrap()).unwrap();
        handle.mark_content_ready();
        (handle, server, key)
    }

    #[test]
    fn incompatible_peer_snapshot_preserves_local_work_and_pauses_undo() {
        Owner::new().with(|| {
            let (handle, server, key) = recovery_peers();
            handle
                .with_doc(|doc| doc.set_need(&key, 15))
                .unwrap()
                .unwrap();
            assert!(handle.can_undo());
            // Version-vector encoding may order peers differently after import.
            // Compare causal state, not its non-canonical serialization.
            let version = || handle.with_doc(|doc| doc.inner().oplog_vv()).unwrap();
            let before = version();
            server
                .inner()
                .get_map("meta")
                .insert("schema", 999_i64)
                .unwrap();
            server.commit();
            assert!(matches!(
                handle.import_server_snapshot(&server.export_snapshot().unwrap()),
                Err(DocError::UnsupportedSchema(999))
            ));
            assert!(handle.incompatible());
            assert_eq!(version(), before);
            let saved =
                ListDocument::from_snapshot(&handle.incompatible_snapshot.get_value().unwrap())
                    .unwrap();
            assert_eq!(saved.inner().oplog_vv(), before);
            assert_eq!(saved.row(&key).unwrap().need, 15);
            assert!(!handle.can_undo());
            assert!(!handle.undo());
            assert!(!handle.redo());
            assert_eq!(version(), before);
        });
    }

    fn compact_after(server: &ListDocument) -> Vec<u8> {
        let tip = ListDocument::from_snapshot(&server.export_snapshot().unwrap()).unwrap();
        let name = tip.meta().name;
        tip.rename(&format!("{name} (compaction fixture)")).unwrap();
        tip.rename(&name).unwrap();
        tip.export_shallow().unwrap()
    }

    #[test]
    fn compaction_recovery_swaps_only_after_success_and_resets_undo() {
        Owner::new().with(|| {
            let (handle, server, key) = recovery_peers();
            handle
                .with_doc(|doc| doc.set_need(&key, 15))
                .unwrap()
                .unwrap();
            assert!(handle.can_undo());
            server.add_acquired(&key, 4).unwrap();
            let source = handle.try_version();
            let compact = compact_after(&server);
            assert!(handle.rebase_onto_snapshot(&compact, true).unwrap());
            assert!(
                handle.recovery_persisting.get_untracked(),
                "recovered operations wait for durable storage before sync"
            );
            assert_eq!(handle.rows()[0].need, 15);
            assert_eq!(handle.rows()[0].acquired, 4);
            assert!(!handle.can_undo());
            assert!(!handle.can_redo());
            assert_eq!(handle.replacement_source.get_value(), Some(source));
            assert_eq!(
                handle.recovery_state.get_untracked(),
                RecoveryState::Recovered {
                    dropped_meta: false
                }
            );
            let compact = ListDocument::from_snapshot(&compact).unwrap();
            assert!(
                !compact
                    .import(&handle.export_since(&compact.version()).unwrap())
                    .unwrap()
                    .pending
            );
            assert_eq!(compact.rows(), handle.rows());
            handle
                .with_doc(|doc| doc.set_need(&key, 16))
                .unwrap()
                .unwrap();
            assert!(handle.undo());
            assert_eq!(handle.rows()[0].need, 15);
            assert_eq!(handle.rows()[0].acquired, 4);
            handle.dispose();
        });
    }

    #[test]
    fn conflict_keeps_local_history_and_allows_retry_or_explicit_server_choice() {
        Owner::new().with(|| {
            let (handle, server, key) = recovery_peers();
            handle
                .with_doc(|doc| doc.set_need(&key, 15))
                .unwrap()
                .unwrap();
            server.set_need(&key, 20).unwrap();
            let source = handle.try_version();
            let outbox = handle.outbox.get_untracked();
            let compact = compact_after(&server);
            assert!(handle.import_server_snapshot(&compact).unwrap().pending);
            assert_eq!(handle.try_version(), source);
            assert_eq!(handle.rows()[0].need, 15);
            assert!(handle.rebase_onto_snapshot(&compact, true).is_err());
            assert_eq!(handle.try_version(), source);
            assert_eq!(handle.outbox.get_untracked(), outbox);
            assert!(handle.can_undo());
            assert!(handle.recovery_paused());
            assert_eq!(handle.recovery_server().unwrap().rows()[0].need, 20);
            assert!(handle.replacement_source.get_value().is_none());
            handle
                .with_doc(|doc| doc.set_need(&key, 20))
                .unwrap()
                .unwrap();
            handle.retry_recovery();
            assert!(!handle.recovery_paused());
            assert_eq!(handle.recovery_retry.get_untracked(), 1);
            assert!(!handle.rebase_onto_snapshot(&compact, true).unwrap());
            assert_eq!(handle.rows()[0].need, 20);
            handle
                .with_doc(|doc| doc.set_need(&key, 25))
                .unwrap()
                .unwrap();
            let server = ListDocument::from_snapshot(&compact).unwrap();
            server.set_need(&key, 30).unwrap();
            assert!(
                handle
                    .rebase_onto_snapshot(&server.export_shallow().unwrap(), true)
                    .is_err()
            );
            handle.use_server_version();
            assert!(!handle.recovery_paused());
            assert_eq!(handle.rows()[0].need, 30);
            assert!(!handle.can_undo());
            assert!(handle.outbox.get_untracked().is_empty());
            handle.purge();
            assert!(handle.recovery_server().is_none());
            handle.dispose();
        });
    }

    #[test]
    fn export_failure_keeps_edits_and_retry_does_not_claim_durability() {
        // Native tests keep this browser-style handle on one thread under an
        // explicit owner; it is never constructed by an SSR render.
        let owner = Owner::new();
        owner.with(|| {
            let handle = ListDocHandle::open(1, 1);
            handle
                .with_doc(|doc| doc.rename("Offline title"))
                .unwrap()
                .unwrap();
            let version = handle.try_version();
            let outbox = handle.outbox.get_untracked();
            assert!(!outbox.is_empty());
            handle.save_state.set(SaveState::Saved);

            // Fault-inject the exporter, not the storage writer: no snapshot
            // may cross the persistence boundary on this path.
            assert!(handle.prepare_save(|_| Err(DocError::Version)).is_none());
            assert_eq!(handle.save_state.get_untracked(), SaveState::Failed);
            assert_eq!(handle.try_version(), version);
            assert_eq!(handle.outbox.get_untracked(), outbox);
            assert_eq!(handle.meta().name, "Offline title");

            let snapshot = handle.prepare_save(ListDocument::export_snapshot).unwrap();
            let recovered = ListDocument::from_snapshot(&snapshot).unwrap();
            assert_eq!(recovered.meta().name, "Offline title");
            assert!(!recovered.is_ahead_of(&version));
            assert!(!handle.is_ahead_of(&recovered.version()));
            assert_eq!(handle.save_state.get_untracked(), SaveState::Pending);
            assert_eq!(handle.outbox.get_untracked(), outbox);
            handle.dispose();
        });
    }

    #[test]
    fn revoked_or_disposed_handle_never_attempts_export() {
        let owner = Owner::new();
        owner.with(|| {
            let handle = ListDocHandle::open(1, 1);
            handle.purged.set(true);
            assert!(handle.prepare_save(|_| panic!("revoked export")).is_none());
            handle.dispose();
            assert!(handle.prepare_save(|_| panic!("disposed export")).is_none());
        });
    }
}
