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
    /// This user may not read or write this list. The local copy is no
    /// longer trustworthy to keep syncing and must be purged.
    Denied,
    /// The socket has no session at all ("sign in to edit lists"). This says
    /// nothing about whether the user still has the list, so the local copy
    /// must be kept — signing back in has to resume where they left off —
    /// while sync stops.
    NotSignedIn,
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
///
/// `"sign in to edit lists"` and `"Insufficient permissions"` are kept
/// apart at classification time rather than guessed at afterwards: the
/// first means the session lapsed and the snapshot must be **kept** (or the
/// user loses every edit they made offline the moment their cookie
/// expires), the second means this user may not have the list at all and it
/// must be purged. The page used to tell them apart by asking whether the
/// login resource had resolved to a user, but that resource never refetches,
/// so a cookie that lapsed mid-session still looked signed in and purged.
pub fn classify_error(message: &str) -> ErrorKind {
    if message == "sign in to edit lists" {
        ErrorKind::NotSignedIn
    } else if message.contains("Insufficient permissions") {
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
/// Per `ListDocSubscribed { version: V, payload }`, first import the payload
/// (a failed import is fatal for this reply: status `"offline"`, logged,
/// nothing sent). A **pending** import (`ImportReport::pending`, F1) means
/// the bytes were parked on history this document doesn't have and the doc
/// was *not* actually brought up to date:
///
/// - payload was `Snapshot` and pending — replace the document outright
///   with that snapshot and re-apply local rows as fresh ops
///   (`ListDocHandle::rebase_onto_snapshot`), then fall through into the
///   logic below with the same `V` (the snapshot is treated as consumed, so
///   the `(same_version, Some(bytes))` branch below cannot fire a second
///   time for it).
/// - payload was `Updates` and pending — an `Updates` payload alone can't
///   rebuild the document. Arm `force_empty_version` and resubscribe
///   (deferred) so the next handshake asks for a full `Snapshot`; status
///   `"reconnecting"`, return without reaching "live".
///
/// Otherwise (not pending, or `UpToDate`):
///
/// - not ahead of `V` — converged; both guards below are cleared.
/// - ahead of `V`, and `V` is not the version we last diffed against — send
///   `export_since(V)` and remember `last_diff_version = V`.
/// - ahead of `V`, `V` *is* `last_diff_version`, and the payload was a
///   (non-pending) `Snapshot` — the server rejected our diff for exactly
///   this version and answered with a fresh snapshot, so resending the same
///   bytes would loop forever, each cycle costing the server a whole
///   snapshot. Rebase the local rows onto that snapshot
///   (`ListDocHandle::rebase_onto_snapshot`), dropping a local meta change
///   if a `MetaForbidden` was seen since the last successful handshake,
///   send `export_since(V)` once more, and remember `rebased_for = V`.
/// - ahead of `V`, `rebased_for == V`, **and the payload was a `Snapshot`**
///   (F2) — the rebased operations were rejected too (a `MissingHistory`
///   resync always answers with a fresh Snapshot, so this is the only way
///   to legitimately see the same `V` twice with `rebased_for` already set).
///   Give up: status `"offline"`, logged, nothing sent. This bounds the
///   exchange at two rounds per server version. The same `V` arriving again
///   with `Updates`/`UpToDate` instead — e.g. a lost send followed by a
///   reconnect replaying the same version — is not a rejection and falls
///   through to the normal diff-send branch above instead of giving up.
///
/// A `ListDocSubscribed` for a different version clears `rebased_for`, so a
/// document that recovers is not stuck in the give-up state.
///
/// A pending relayed `ListDocUpdate` (outside a handshake) gets the same
/// F1 treatment: force a snapshot resync instead of calling
/// `on_remote_change` or reporting "live" on a doc that didn't actually
/// converge.
///
/// `on_denied` fires, after status is set to `"offline"`, for a scoped
/// error that means this client should stop trying to sync this document
/// (`ErrorKind::Denied` / `ErrorKind::NotFound` / `ErrorKind::NotSignedIn`);
/// Task 8 wires the first two to `handle.purge()` and the last to a plain
/// `close()` that keeps the snapshot, both plus dropping this subscription. **All three callbacks
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
    //
    // M1: comparing `last_diff_version`/`rebased_for` to an incoming
    // `version` is a byte-equality check on the *encoded* version vector,
    // not a semantic "same causal version" check. That's sound only because
    // the server always recomputes the bytes it sends from a freshly
    // re-serialized stored snapshot/doc state — never replays the bytes it
    // received — so two handshake replies that are causally the same
    // version (in particular, the reply to a rejected diff and the reply to
    // the rebase sent right after) always re-encode to the identical byte
    // string. If the server ever started caching or forwarding a
    // client-supplied version's raw bytes, this comparison would need to
    // become a semantic one instead.
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
        move |message| {
            if handle.is_closed_or_disposed() {
                return;
            }
            match message {
            ServerClient::ListDocSubscribed {
                version, payload, ..
            } => {
                // `snapshot` is `Some(bytes)` only when the reply carried a
                // Snapshot AND that snapshot fully merged (not pending) —
                // i.e. it's still available for the "server rejected our
                // diff" rebase branch below. A pending Snapshot is handled
                // right here instead (F1) and does not flow into that
                // branch a second time.
                // Set when a pending Snapshot import replaced the document (F1).
                let mut pending_rebased = false;
                let snapshot = match payload {
                    ListDocPayload::Snapshot(bytes) => {
                        let report = match handle.import(&bytes) {
                            Ok(report) => report,
                            Err(error) => {
                                handle.set_status("offline");
                                log::error!(
                                    "list {list_id}: server snapshot did not import: {error}"
                                );
                                return;
                            }
                        };
                        if report.pending {
                            // F1: the merge parked ops because they depend
                            // on history this document doesn't have (e.g.
                            // an idle client after a server compaction) —
                            // the doc was NOT brought up to date by that
                            // import. Recover the same way a rejected diff
                            // does: replace the document outright with the
                            // server's snapshot and re-apply local rows as
                            // fresh ops, then fall through into the normal
                            // "am I ahead" logic with this same `version` so
                            // the freshly rebased rows still get sent.
                            let keep_meta = !meta_forbidden.get();
                            if let Err(error) = handle.rebase_onto_snapshot(&bytes, keep_meta) {
                                handle.set_status("offline");
                                log::error!(
                                    "list {list_id}: rebase onto pending snapshot failed: {error}"
                                );
                                return;
                            }
                            meta_forbidden.set(false);
                            pending_rebased = true;
                            None
                        } else {
                            Some(bytes)
                        }
                    }
                    ListDocPayload::Updates(bytes) => {
                        let report = match handle.import(&bytes) {
                            Ok(report) => report,
                            Err(error) => {
                                handle.set_status("offline");
                                log::error!(
                                    "list {list_id}: server updates did not import: {error}"
                                );
                                return;
                            }
                        };
                        if report.pending {
                            // F1: same recovery, but an `Updates` payload
                            // alone can't rebuild the document — force the
                            // next handshake to ask for a full `Snapshot`
                            // instead, and don't report "live" on a doc that
                            // just silently dropped part of this import.
                            handle.set_status("reconnecting");
                            // Already armed means a snapshot resync is in flight;
                            // a burst of pending imports schedules one, not N.
                            if !force_empty_version.replace(true) {
                                let slot = weak_slot.clone();
                                let flag = force_empty_version.clone();
                                defer(move || resubscribe_forcing_snapshot(&slot, &flag));
                            }
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
                    if pending_rebased {
                        // This handshake already rebased onto `version`, so a
                        // later rejection at the same version gives up instead
                        // of rebasing again: two rounds per server version.
                        *rebased_for.borrow_mut() = Some(version.clone());
                    }
                    // F2: only give up when the server has actually
                    // rejected our history — signalled by answering the
                    // same version we already rebased for with a fresh
                    // `Snapshot` again (a `MissingHistory` resync always
                    // answers with a Snapshot). A lost send followed by a
                    // reconnect can also replay the same version, but with
                    // `Updates`/`UpToDate`, and must fall through to a
                    // normal diff retry instead of stranding the client
                    // "offline" over nothing.
                    if rebased_for.borrow().as_deref() == Some(version.as_slice())
                        && snapshot.is_some()
                    {
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
                            // M4: only record the rebase (and clear the
                            // outbox) once the diff for it actually went
                            // out — an export failure here must not make a
                            // later reply believe this version was already
                            // answered.
                            let diff = match handle.export_since(&version) {
                                Ok(diff) => diff,
                                Err(error) => {
                                    handle.set_status("offline");
                                    log::error!(
                                        "list {list_id}: export_since after rebase failed: {error}"
                                    );
                                    return;
                                }
                            };
                            sender.send_list_doc_update(list_id, diff);
                            *rebased_for.borrow_mut() = Some(version.clone());
                            // That diff covers every re-applied operation.
                            handle.outbox.set(Vec::new());
                        }
                        _ => {
                            // M4: same ordering — don't record
                            // `last_diff_version` unless the diff was
                            // actually sent.
                            let diff = match handle.export_since(&version) {
                                Ok(diff) => diff,
                                Err(error) => {
                                    handle.set_status("offline");
                                    log::error!("list {list_id}: export_since failed: {error}");
                                    return;
                                }
                            };
                            sender.send_list_doc_update(list_id, diff);
                            // The handshake diff covers everything the outbox
                            // held up to this point; anything committed
                            // locally after this reply started will still push
                            // through the drain Effect below.
                            handle.outbox.set(Vec::new());
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
                let report = match handle.import(&update) {
                    Ok(report) => report,
                    Err(error) => {
                        handle.set_status("offline");
                        log::error!("list {list_id}: relayed update did not import: {error}");
                        return;
                    }
                };
                if report.pending {
                    // F1: this relayed update depended on history we don't
                    // have — importing it did not actually bring the
                    // document up to date, so don't run `on_remote_change`
                    // or claim "live". Force the next handshake to answer
                    // with a full Snapshot and resubscribe to trigger it.
                    handle.set_status("reconnecting");
                    // See the handshake arm: one resync per burst.
                    if !force_empty_version.replace(true) {
                        let slot = weak_slot.clone();
                        let flag = force_empty_version.clone();
                        defer(move || resubscribe_forcing_snapshot(&slot, &flag));
                    }
                    return;
                }
                let on_remote_change = on_remote_change.clone();
                defer(move || {
                    if !handle.is_closed_or_disposed() {
                        on_remote_change();
                    }
                });
                handle.set_status("live");
            }
            ServerClient::Stale { .. } => {
                handle.set_status("reconnecting");
                // Spec section 5: a stale subscription is re-handshaked, not
                // merely reported.
                let slot = weak_slot.clone();
                let on_stale = on_stale.clone();
                defer(move || {
                    if handle.is_closed_or_disposed() {
                        return;
                    }
                    resubscribe(&slot);
                    on_stale();
                });
            }
            ServerClient::Error { message } => {
                let kind = classify_error(&message);
                match kind {
                    ErrorKind::Denied | ErrorKind::NotFound | ErrorKind::NotSignedIn => {
                        handle.set_status("offline");
                        let on_denied = on_denied.clone();
                        defer(move || {
                            if !handle.is_closed_or_disposed() {
                                on_denied(kind);
                            }
                        });
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
                        let flag = force_empty_version.clone();
                        defer(move || resubscribe_forcing_snapshot(&slot, &flag));
                    }
                    ErrorKind::MissingHistory | ErrorKind::Transient => {
                        log::warn!("list {list_id} sync: {message}");
                    }
                }
            }
            _ => {}
            }
        },
    );
    *slot.borrow_mut() = Some(subscription);

    let sender = realtime;
    let drain = Effect::new(move |_| {
        // `try_get`, not `get`: the page disposes a handle once it has been
        // superseded, and this Effect is only disposed when the owning
        // `SyncSubscription` is dropped — which happens a tick later.
        let Some(pending) = handle.outbox.try_get() else {
            return;
        };
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
/// Returns whether the message actually went out (see
/// `RealtimeSubscription::resubscribe`); `false` both when there's no
/// subscription left to resend and when the socket wasn't open to send on.
fn resubscribe(slot: &Weak<RefCell<Option<RealtimeSubscription>>>) -> bool {
    if let Some(slot) = slot.upgrade()
        && let Some(subscription) = slot.borrow().as_ref()
    {
        subscription.resubscribe()
    } else {
        false
    }
}

/// M2: `resubscribe` after arming `force_empty_version` (the caller must set
/// the flag to `true` before calling this). The subscription's message
/// factory reads and consumes that flag (`replace(false)`) every time it
/// runs — including when `resubscribe` itself fails to actually send,
/// e.g. because the socket isn't open right now. In that case the flag's
/// effect never reached the server, so the *next* replay (a later
/// `resubscribe`, or the reconnect's `onopen` replay) would silently ask for
/// a normal diff instead of the snapshot this caller needed. Re-arm the flag
/// whenever the send didn't go through.
fn resubscribe_forcing_snapshot(
    slot: &Weak<RefCell<Option<RealtimeSubscription>>>,
    flag: &Rc<Cell<bool>>,
) {
    if !resubscribe(slot) {
        flag.set(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_error_anonymous_update_is_not_signed_in() {
        assert_eq!(
            classify_error("sign in to edit lists"),
            ErrorKind::NotSignedIn
        );
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
