//! The handshake and steady state for one document on the realtime socket
//! (spec section 5). Offline edits need no queue: every reconnect re-runs
//! the handshake with the *current* local version (see
//! `RealtimeClient::subscribe_list_doc`), and the server always answers
//! with a superset of what the client has, so the client sends whatever the
//! server is still missing.

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
    _subscription: RealtimeSubscription,
    drain: Option<Effect<LocalStorage>>,
}

impl Drop for SyncSubscription {
    fn drop(&mut self) {
        if let Some(effect) = self.drain.take() {
            effect.dispose();
        }
    }
}

/// Subscribe with the local version, apply what the server sends, send what
/// it lacks, then relay local commits and import remote ones. Every
/// `ListDocSubscribed` reply is treated as a (re)handshake, not only the
/// first: the server also sends one after a `MissingHistory` resync on the
/// update path, reusing the same subscription id (spec section 5;
/// `ultros/src/web/api/real_time_data.rs`). `on_denied` fires, after status
/// is set to `"offline"`, for a scoped error that means this client should
/// stop trying to sync this document (`ErrorKind::Denied` /
/// `ErrorKind::NotFound`); Task 8 wires it to `handle.purge()` plus dropping
/// this subscription. Every other error kind only logs and leaves the local
/// document as is.
pub fn start(
    handle: ListDocHandle,
    realtime: RealtimeClient,
    on_stale: impl Fn() + Clone + 'static,
    on_remote_change: impl Fn() + Clone + 'static,
    on_denied: impl Fn(ErrorKind) + Clone + 'static,
) -> SyncSubscription {
    handle.set_status("connecting");
    let list_id = handle.list_id;
    let sender = realtime.clone();
    let subscription = realtime.subscribe_list_doc(
        list_id,
        move || handle.version(),
        move |message| match message {
            ServerClient::ListDocSubscribed {
                version, payload, ..
            } => {
                let imported = match payload {
                    ListDocPayload::Snapshot(bytes) | ListDocPayload::Updates(bytes) => {
                        handle.import(&bytes).is_ok()
                    }
                    ListDocPayload::UpToDate => true,
                };
                if imported && handle.is_ahead_of(&version) {
                    // The handshake diff covers everything the outbox held
                    // up to this point; anything committed locally after
                    // this reply started will still push through the
                    // drain Effect below.
                    handle.outbox.set(Vec::new());
                    if let Ok(diff) = handle.export_since(&version) {
                        sender.send_list_doc_update(list_id, diff);
                    }
                }
                handle.set_status("live");
                handle.save_now();
            }
            ServerClient::ListDocUpdate { update, .. } => {
                if handle.import(&update).is_ok() {
                    on_remote_change();
                }
                handle.set_status("live");
            }
            ServerClient::Stale { .. } => {
                handle.set_status("reconnecting");
                on_stale();
            }
            ServerClient::Error { message } => {
                let kind = classify_error(&message);
                match kind {
                    ErrorKind::Denied | ErrorKind::NotFound => {
                        handle.set_status("offline");
                        on_denied(kind);
                    }
                    ErrorKind::MetaForbidden | ErrorKind::MissingHistory | ErrorKind::Transient => {
                        log::warn!("list {list_id} sync: {message}");
                    }
                }
            }
            _ => {}
        },
    );

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
        _subscription: subscription,
        drain: Some(drain),
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
