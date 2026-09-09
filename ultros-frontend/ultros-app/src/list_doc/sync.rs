//! The handshake and steady state for one document on the realtime socket
//! (spec section 5). Offline edits need no queue: every reconnect re-runs
//! the handshake with the *current* local version (see
//! `RealtimeClient::subscribe_list_doc`), and the server always answers
//! with a superset of what the client has, so the client sends whatever the
//! server is still missing.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use leptos::prelude::*;
use ultros_api_types::websocket::{ListDocPayload, ServerClient};

use crate::list_doc::handle::ListDocHandle;
use crate::ws::realtime::{RealtimeClient, RealtimeSubscription};

/// How a scoped `ServerClient::Error` for this document should be handled.
/// Pinned to the server's error text (`ultros/src/web/api/real_time_data.rs`,
/// `ultros-db/src/list_doc.rs`, `ultros-db/src/lists.rs`) — see
/// `classify_error`'s doc comment for the exact strings and why a wording
/// change on the server silently breaks this classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// No permission to read or write this list (including "not signed
    /// in"). The local copy is no longer trustworthy to keep syncing.
    Denied,
    /// The list itself is gone.
    NotFound,
    /// A non-owner tried to change the list's name or scope; the row data
    /// is untouched and still fine to sync.
    MetaForbidden,
    /// The update path's own resync fallback text, seen only when this
    /// socket had no `SubscribeListDoc` subscription to route a
    /// `MissingHistory` resync through (the normal case is a fresh
    /// `ListDocSubscribed` on the same subscription, handled below without
    /// ever surfacing an `Error`).
    MissingHistory,
    /// Anything else: transport hiccup, unexpected server text, etc.
    Transient,
}

/// Classifies a `ServerClient::Error` message's text for a list-document
/// subscription. This is coupled to the exact strings the server produces
/// today:
/// - `"sign in to edit lists"` — anonymous update attempt.
/// - `"list {id}: Insufficient permissions to read/edit list"` — `ListError::Forbidden`.
/// - `"list {id}: List not found"` — `ListError::NotFound`.
/// - `"list {id}: only the list owner can change its name or scope"` — `ListDocError::MetaForbidden`.
/// - `"list {id}: update depends on history this server does not have; resync from a snapshot"` — `ListDocError::MissingHistory`.
///
/// Any other text (including future/renamed server errors) classifies as
/// `Transient` and is logged without disturbing the local document.
pub fn classify_error(message: &str) -> ErrorKind {
    if message == "sign in to edit lists" || message.contains("Insufficient permissions") {
        ErrorKind::Denied
    } else if message.contains("List not found") {
        ErrorKind::NotFound
    } else if message.contains("only the list owner can change its name or scope") {
        ErrorKind::MetaForbidden
    } else if message.contains("resync from a snapshot") {
        ErrorKind::MissingHistory
    } else {
        ErrorKind::Transient
    }
}

/// The live handles for one document's realtime sync. Dropping this stops
/// both halves: the `RealtimeSubscription` unsubscribes on the socket, and
/// the outbox-drain `Effect` is disposed so it never fires again. Task 8
/// drops this (closing the old handle+subscription) on account switch or
/// when the page navigates away from the list.
pub struct SyncSubscription {
    /// The socket subscription lives behind an `Rc` because the message
    /// handler needs a `Weak` back to it in order to re-handshake
    /// (`Stale`, `MetaForbidden`). This is the only strong reference, so
    /// dropping `SyncSubscription` still unsubscribes immediately.
    _subscription: Rc<RefCell<Option<RealtimeSubscription>>>,
    drain: Option<Effect<LocalStorage>>,
}

impl Drop for SyncSubscription {
    fn drop(&mut self) {
        if let Some(effect) = self.drain.take() {
            effect.dispose();
        }
    }
}

/// Run `task` after the current dispatch returns.
///
/// `RealtimeClient::dispatch_message` invokes a handler while
/// `inner.handlers.borrow()` is live, so anything that touches the
/// subscription table from inside a handler double-borrows: dropping a
/// `RealtimeSubscription` takes `handlers.borrow_mut()`, which would panic
/// and abort the wasm module. Every caller callback and every resubscribe
/// therefore runs from a deferred task instead, where no borrow is held.
fn defer(task: impl FnOnce() + 'static) {
    leptos::task::spawn_local(async move {
        task();
    });
}

/// Subscribe with the local version, apply what the server sends, send what
/// it lacks, then relay local commits and import remote ones. Every
/// `ListDocSubscribed` reply is treated as a (re)handshake, not only the
/// first: the server also sends one after a `MissingHistory` resync on the
/// update path, reusing the same subscription id (spec section 5;
/// `ultros/src/web/api/real_time_data.rs`).
///
/// # The handshake state machine
///
/// Per `ListDocSubscribed { version: V, payload }`, after importing the
/// payload (a failed import is fatal for this reply: status `"offline"`,
/// logged, nothing sent):
///
/// - not ahead of `V` — converged; both guards below are cleared.
/// - ahead of `V`, and `V` is not the version we last diffed against — send
///   `export_since(V)` and remember `last_diff_version = V`.
/// - ahead of `V`, `V` *is* `last_diff_version`, and the payload was a
///   `Snapshot` — the server rejected our diff for exactly this version and
///   answered with a fresh snapshot, so resending the same bytes would loop
///   forever, each cycle costing the server a whole snapshot. Rebase the
///   local rows onto that snapshot (`ListDocHandle::rebase_onto_snapshot`),
///   dropping a local meta change if a `MetaForbidden` was seen since the
///   last successful handshake, send `export_since(V)` once more, and
///   remember `rebased_for = V`.
/// - ahead of `V` and `rebased_for == V` — the rebased operations were
///   rejected too. Give up: status `"offline"`, logged, nothing sent. This
///   bounds the exchange at two rounds per server version.
///
/// A `ListDocSubscribed` for a different version clears `rebased_for`, so a
/// document that recovers is not stuck in the give-up state.
///
/// `on_denied` fires, after status is set to `"offline"`, for a scoped
/// error that means this client should stop trying to sync this document
/// (`ErrorKind::Denied` / `ErrorKind::NotFound`); Task 8 wires it to
/// `handle.purge()` plus dropping this subscription. **All three callbacks
/// run after `dispatch_message` has returned**, so dropping the
/// `SyncSubscription` from inside one of them is safe.
pub fn start(
    handle: ListDocHandle,
    realtime: RealtimeClient,
    on_stale: impl Fn() + 'static,
    on_remote_change: impl Fn() + 'static,
    on_denied: impl Fn(ErrorKind) + 'static,
) -> SyncSubscription {
    handle.set_status("connecting");
    let list_id = handle.list_id;
    let on_stale: Rc<dyn Fn()> = Rc::new(on_stale);
    let on_remote_change: Rc<dyn Fn()> = Rc::new(on_remote_change);
    let on_denied: Rc<dyn Fn(ErrorKind)> = Rc::new(on_denied);

    // Filled in right after `subscribe_list_doc` returns; the handler only
    // ever sees it through a `Weak`, so the `Rc` in `SyncSubscription` stays
    // the sole owner and dropping that really does unsubscribe.
    let slot: Rc<RefCell<Option<RealtimeSubscription>>> = Rc::new(RefCell::new(None));
    let weak_slot = Rc::downgrade(&slot);

    // The last server version we answered with a diff, and the version we
    // already rebased for: see the state machine above.
    let last_diff_version: Rc<RefCell<Option<Vec<u8>>>> = Rc::new(RefCell::new(None));
    let rebased_for: Rc<RefCell<Option<Vec<u8>>>> = Rc::new(RefCell::new(None));
    // Set when the server refuses a non-owner's meta change; cleared by the
    // rebase that drops that change.
    let meta_forbidden = Rc::new(Cell::new(false));
    // Makes the next handshake message claim an empty version, which forces
    // the server to answer with a `Snapshot` rather than a diff.
    let force_empty_version = Rc::new(Cell::new(false));

    let sender = realtime.clone();
    let version_flag = force_empty_version.clone();
    let subscription = realtime.subscribe_list_doc(
        list_id,
        move || {
            if version_flag.replace(false) {
                Vec::new()
            } else {
                // Never `version()`: this factory outlives the page on a
                // reconnect replay, and reading a disposed `StoredValue`
                // would abort the module (M10).
                handle.try_version()
            }
        },
        move |message| match message {
            ServerClient::ListDocSubscribed {
                version, payload, ..
            } => {
                let snapshot = match payload {
                    ListDocPayload::Snapshot(bytes) => {
                        if let Err(error) = handle.import(&bytes) {
                            handle.set_status("offline");
                            log::error!("list {list_id}: server snapshot did not import: {error}");
                            return;
                        }
                        Some(bytes)
                    }
                    ListDocPayload::Updates(bytes) => {
                        if let Err(error) = handle.import(&bytes) {
                            handle.set_status("offline");
                            log::error!("list {list_id}: server updates did not import: {error}");
                            return;
                        }
                        None
                    }
                    ListDocPayload::UpToDate => None,
                };

                if handle.is_ahead_of(&version) {
                    let same_version =
                        last_diff_version.borrow().as_deref() == Some(version.as_slice());
                    if !same_version {
                        // A different server version is a fresh start.
                        *rebased_for.borrow_mut() = None;
                    }
                    if rebased_for.borrow().as_deref() == Some(version.as_slice()) {
                        handle.set_status("offline");
                        log::error!("list {list_id}: server keeps rejecting local history");
                        return;
                    }
                    match (same_version, snapshot) {
                        (true, Some(bytes)) => {
                            let keep_meta = !meta_forbidden.get();
                            if let Err(error) = handle.rebase_onto_snapshot(&bytes, keep_meta) {
                                handle.set_status("offline");
                                log::error!("list {list_id}: rebase onto snapshot failed: {error}");
                                return;
                            }
                            meta_forbidden.set(false);
                            *rebased_for.borrow_mut() = Some(version.clone());
                            if let Ok(diff) = handle.export_since(&version) {
                                sender.send_list_doc_update(list_id, diff);
                            }
                            // That diff covers every re-applied operation.
                            handle.outbox.set(Vec::new());
                        }
                        _ => {
                            // The handshake diff covers everything the outbox
                            // held up to this point; anything committed
                            // locally after this reply started will still push
                            // through the drain Effect below.
                            handle.outbox.set(Vec::new());
                            if let Ok(diff) = handle.export_since(&version) {
                                sender.send_list_doc_update(list_id, diff);
                            }
                            *last_diff_version.borrow_mut() = Some(version.clone());
                        }
                    }
                } else {
                    *last_diff_version.borrow_mut() = None;
                    *rebased_for.borrow_mut() = None;
                }
                handle.set_status("live");
                handle.save_now();
            }
            ServerClient::ListDocUpdate { update, .. } => {
                if let Err(error) = handle.import(&update) {
                    handle.set_status("offline");
                    log::error!("list {list_id}: relayed update did not import: {error}");
                    return;
                }
                let on_remote_change = on_remote_change.clone();
                defer(move || on_remote_change());
                handle.set_status("live");
            }
            ServerClient::Stale { .. } => {
                handle.set_status("reconnecting");
                // Spec section 5: a stale subscription is re-handshaked, not
                // merely reported.
                let slot = weak_slot.clone();
                let on_stale = on_stale.clone();
                defer(move || {
                    resubscribe(&slot);
                    on_stale();
                });
            }
            ServerClient::Error { message } => {
                let kind = classify_error(&message);
                match kind {
                    ErrorKind::Denied | ErrorKind::NotFound => {
                        handle.set_status("offline");
                        let on_denied = on_denied.clone();
                        defer(move || on_denied(kind));
                    }
                    ErrorKind::MetaForbidden => {
                        log::warn!("list {list_id} sync: {message}");
                        // The rejected meta operation is still in the
                        // document, and every later `export_since` blob would
                        // carry it, so the row edits riding along with it
                        // would be rejected too. Force a snapshot handshake;
                        // the second identical reply rebases the rows onto it
                        // *without* the meta change.
                        meta_forbidden.set(true);
                        force_empty_version.set(true);
                        let slot = weak_slot.clone();
                        defer(move || resubscribe(&slot));
                    }
                    ErrorKind::MissingHistory | ErrorKind::Transient => {
                        log::warn!("list {list_id} sync: {message}");
                    }
                }
            }
            _ => {}
        },
    );
    *slot.borrow_mut() = Some(subscription);

    let sender = realtime;
    let drain = Effect::new(move |_| {
        let pending = handle.outbox.get();
        if pending.is_empty() {
            return;
        }
        for update in pending {
            sender.send_list_doc_update(list_id, update);
        }
        // Cleared unconditionally, success or not: the outbox is not a
        // delivery queue (spec section 5 — no offline queue). Whatever a
        // failed send didn't get through is still sitting in the document
        // itself, and the next handshake's `export_since(server_version)`
        // covers it along with anything else the server lacks. Leaving
        // unsent bytes in the outbox would only resend them out of order
        // relative to the handshake's diff without making delivery any
        // more reliable.
        handle.outbox.set(Vec::new());
    });

    SyncSubscription {
        _subscription: slot,
        drain: Some(drain),
    }
}

/// Re-send the handshake for a subscription that may already have been
/// dropped (the page navigated away while a deferred task was queued).
fn resubscribe(slot: &Weak<RefCell<Option<RealtimeSubscription>>>) {
    if let Some(slot) = slot.upgrade()
        && let Some(subscription) = slot.borrow().as_ref()
    {
        subscription.resubscribe();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_error_anonymous_update_is_denied() {
        assert_eq!(classify_error("sign in to edit lists"), ErrorKind::Denied);
    }

    #[test]
    fn classify_error_forbidden_read_is_denied() {
        assert_eq!(
            classify_error("list 9: Insufficient permissions to read list"),
            ErrorKind::Denied
        );
    }

    #[test]
    fn classify_error_forbidden_edit_is_denied() {
        assert_eq!(
            classify_error("list 9: Insufficient permissions to edit list"),
            ErrorKind::Denied
        );
    }

    #[test]
    fn classify_error_not_found() {
        assert_eq!(
            classify_error("list 9: List not found"),
            ErrorKind::NotFound
        );
    }

    #[test]
    fn classify_error_meta_forbidden() {
        assert_eq!(
            classify_error("list 9: only the list owner can change its name or scope"),
            ErrorKind::MetaForbidden
        );
    }

    #[test]
    fn classify_error_missing_history() {
        assert_eq!(
            classify_error(
                "list 9: update depends on history this server does not have; resync from a snapshot"
            ),
            ErrorKind::MissingHistory
        );
    }

    #[test]
    fn classify_error_unknown_text_is_transient() {
        assert_eq!(
            classify_error("too many active subscriptions, max is 20"),
            ErrorKind::Transient
        );
    }
}
