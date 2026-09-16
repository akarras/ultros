//! Notification inbox: pure `AlertEvent` → `InboxItem` conversion and
//! merge/bookkeeping logic, plus the reactive [`Inbox`] store that backs the
//! sidebar inbox UI (built in a later task).
//!
//! Two sources feed the inbox:
//! - `server`: events fetched from `GET /api/v1/alerts/events` and pushed
//!   live over the websocket (`ServerClient::Notification`). Only populated
//!   for signed-in users.
//! - `local`: client-only entries (e.g. a guest alert rule firing with no
//!   account to persist to), kept in `localStorage` so they survive a
//!   reload.
//!
//! `merge_inbox` combines both into the single list the UI renders.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use codee::string::JsonSerdeCodec;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_use::storage::{UseStorageOptions, use_local_storage_with_options};
use serde::{Deserialize, Serialize};
use ultros_api_types::alert::{AlertEvent, MarkAlertEventsReadRequest};

use crate::api::mark_alert_events_read;

/// `localStorage` key for the client-only half of the inbox.
pub const LOCAL_INBOX_KEY: &str = "ultros.inbox.local.v1";
/// Local entries are capped and oldest-first evicted so a guest who never
/// clears their browser doesn't grow this list forever.
pub const LOCAL_INBOX_CAP: usize = 100;

/// Identifies one inbox entry, either a server-persisted `AlertEvent` row or
/// a client-only local hit.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InboxId {
    Server(i64),
    Local(String),
}

/// One row in the notification inbox, already resolved to display-ready
/// strings — no `AlertEvent` or item lookups needed at render time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InboxItem {
    pub id: InboxId,
    pub title: String,
    pub body: String,
    pub url: Option<String>,
    pub item_id: i32,
    pub at: DateTime<Utc>,
    pub read: bool,
}

/// Builds an [`InboxItem`] from a server `AlertEvent`.
///
/// `title` falls back to the item's display name (via `item_name`) when the
/// server didn't provide one; `body` falls back to `fallback_body`, which the
/// caller supplies pre-translated (this crate has no user-facing strings of
/// its own — see the module docs on [`crate::global_state`]). `url` falls
/// back to the item's page. `read` mirrors whether `read_at` is set.
pub fn inbox_item_from_event(
    event: &AlertEvent,
    item_name: impl Fn(i32) -> String,
    fallback_body: impl Fn(&AlertEvent) -> String,
) -> InboxItem {
    let title = event
        .title
        .clone()
        .unwrap_or_else(|| item_name(event.item_id));
    let body = event.body.clone().unwrap_or_else(|| fallback_body(event));
    let url = event
        .click_url
        .clone()
        .or_else(|| Some(format!("/item/{}", event.item_id)));
    InboxItem {
        id: InboxId::Server(event.id),
        title,
        body,
        url,
        item_id: event.item_id,
        at: event.fired_at,
        read: event.read_at.is_some(),
    }
}

/// Merges the server and local halves of the inbox into the single
/// newest-first list the UI renders.
///
/// - Entries sharing an `id` collapse to the first occurrence (`server` is
///   scanned first, so a server entry always wins over a `Local` one that
///   happens to share an id — which can't normally happen since the two
///   variants don't overlap, but keeps the function well-defined if callers
///   ever pass overlapping input).
/// - A `Local` entry is dropped outright when an already-accepted entry
///   (almost always the server-persisted version of the same alert firing)
///   matches it on `(item_id, body)` and lands within 2 seconds of it — the
///   optimistic local hit and the event that later arrives from the server
///   are the same real-world firing and should show up once, not twice.
pub fn merge_inbox(server: &[InboxItem], local: &[InboxItem]) -> Vec<InboxItem> {
    let mut merged: Vec<InboxItem> = Vec::with_capacity(server.len() + local.len());
    let mut seen_ids: HashSet<InboxId> = HashSet::with_capacity(server.len() + local.len());

    for item in server.iter().chain(local.iter()) {
        if !seen_ids.insert(item.id.clone()) {
            continue;
        }
        if matches!(item.id, InboxId::Local(_))
            && merged.iter().any(|existing| {
                existing.item_id == item.item_id
                    && existing.body == item.body
                    && (existing.at - item.at).num_seconds().abs() <= 2
            })
        {
            continue;
        }
        merged.push(item.clone());
    }

    merged.sort_by(|a, b| b.at.cmp(&a.at));
    merged
}

/// Number of unread entries in `items`.
pub fn unread_count(items: &[InboxItem]) -> usize {
    items.iter().filter(|item| !item.read).count()
}

/// Pushes a new local entry to the front (newest-first) of `local`, dropping
/// the oldest entries once `cap` is exceeded.
pub fn push_local_bounded(local: &mut Vec<InboxItem>, item: InboxItem, cap: usize) {
    local.insert(0, item);
    if local.len() > cap {
        local.truncate(cap);
    }
}

/// Flips `read = true` on every item in `items` whose id is listed in `ids`,
/// leaving every other item untouched.
pub fn mark_read(items: &mut [InboxItem], ids: &[InboxId]) {
    for item in items.iter_mut() {
        if ids.contains(&item.id) {
            item.read = true;
        }
    }
}

/// Reactive notification-inbox store: a `server`-fetched signal plus a
/// `localStorage`-backed local signal, merged into `items`/`unread_count`.
///
/// `delay_during_hydration(true)` on the local-storage hook is mandatory —
/// see `ultros-ui-market/src/components/recently_viewed.rs` for why reading
/// `localStorage` synchronously during client setup races hydration and
/// panics. `hydrated` additionally gates `items`/`unread_count` on an
/// `Effect` having run at least once client-side, so the SSR render and the
/// first client render always agree (same idiom as
/// `global_state::changelog::use_whats_new_indicator`) — without it, a guest
/// with local hits would render a different `items`/`unread_count` on the
/// server (always empty) vs. the client's first pass (as soon as the storage
/// hook resolves), which is exactly the class of mismatch that panics
/// tachys' hydration walker.
#[derive(Clone, Copy)]
pub struct Inbox {
    server: RwSignal<Vec<InboxItem>>,
    local_write: WriteSignal<Vec<InboxItem>>,
    items: Signal<Vec<InboxItem>>,
    unread_count: Signal<usize>,
}

impl Default for Inbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Inbox {
    pub fn new() -> Self {
        let server = RwSignal::new(Vec::<InboxItem>::new());
        let (local_read, local_write, _delete_fn) =
            use_local_storage_with_options::<Vec<InboxItem>, JsonSerdeCodec>(
                LOCAL_INBOX_KEY,
                UseStorageOptions::default().delay_during_hydration(true),
            );

        let hydrated = RwSignal::new(false);
        Effect::new(move |_| {
            hydrated.set(true);
        });

        let items = Signal::derive(move || {
            if hydrated.get() {
                merge_inbox(&server.get(), &local_read.get())
            } else {
                merge_inbox(&server.get(), &[])
            }
        });
        let unread_count_signal = Signal::derive(move || items.with(|items| unread_count(items)));

        Self {
            server,
            local_write,
            items,
            unread_count: unread_count_signal,
        }
    }

    pub fn items(&self) -> Signal<Vec<InboxItem>> {
        self.items
    }

    pub fn unread_count(&self) -> Signal<usize> {
        self.unread_count
    }

    /// Upserts `events` into the server half of the inbox by id. `item_name`
    /// and `fallback_body` are forwarded to [`inbox_item_from_event`] — see
    /// that function's docs for why they're the caller's responsibility.
    pub fn ingest_server_events(
        &self,
        events: Vec<AlertEvent>,
        item_name: impl Fn(i32) -> String,
        fallback_body: impl Fn(&AlertEvent) -> String,
    ) {
        if events.is_empty() {
            return;
        }
        let new_items: Vec<InboxItem> = events
            .iter()
            .map(|event| inbox_item_from_event(event, &item_name, &fallback_body))
            .collect();
        self.server.update(|server| {
            for item in new_items {
                match server.iter_mut().find(|existing| existing.id == item.id) {
                    Some(existing) => *existing = item,
                    None => server.push(item),
                }
            }
        });
    }

    /// Adds a client-only entry (e.g. a guest alert rule firing locally).
    pub fn push_local(&self, item: InboxItem) {
        self.local_write
            .update(|local| push_local_bounded(local, item, LOCAL_INBOX_CAP));
    }

    /// Marks the listed ids read. `Server` ids are flipped optimistically
    /// and also sent to the server; `Local` ids are flipped in local
    /// storage only. Errors from the server call are logged and otherwise
    /// ignored — the optimistic local flip is not reverted.
    pub fn mark_read(&self, ids: Vec<InboxId>) {
        let server_ids: Vec<i64> = ids
            .iter()
            .filter_map(|id| match id {
                InboxId::Server(id) => Some(*id),
                InboxId::Local(_) => None,
            })
            .collect();

        if !server_ids.is_empty() {
            self.server.update(|items| mark_read(items, &ids));
            spawn_local(async move {
                let request = MarkAlertEventsReadRequest {
                    ids: server_ids,
                    up_to_id: None,
                };
                if let Err(error) = mark_alert_events_read(request).await {
                    log::error!("failed to mark alert events read: {error}");
                }
            });
        }

        if ids.iter().any(|id| matches!(id, InboxId::Local(_))) {
            self.local_write.update(|items| mark_read(items, &ids));
        }
    }

    /// Marks every current item read, both locally and (via `up_to_id`) on
    /// the server. Optimistic: never reverted on a failed server call.
    pub fn mark_all_read(&self) {
        let max_server_id = self.server.with(|items| {
            items
                .iter()
                .filter_map(|item| match item.id {
                    InboxId::Server(id) => Some(id),
                    InboxId::Local(_) => None,
                })
                .max()
        });

        self.server.update(|items| {
            for item in items.iter_mut() {
                item.read = true;
            }
        });
        self.local_write.update(|items| {
            for item in items.iter_mut() {
                item.read = true;
            }
        });

        if let Some(up_to_id) = max_server_id {
            spawn_local(async move {
                let request = MarkAlertEventsReadRequest {
                    ids: vec![],
                    up_to_id: Some(up_to_id),
                };
                if let Err(error) = mark_alert_events_read(request).await {
                    log::error!("failed to mark all alert events read: {error}");
                }
            });
        }
    }
}

pub fn provide_inbox() {
    provide_context(Inbox::new());
}

pub fn use_inbox() -> Option<Inbox> {
    use_context::<Inbox>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::alert::AlertEvent;

    fn event(id: i64, item_id: i32, fired_at: DateTime<Utc>) -> AlertEvent {
        AlertEvent {
            id,
            alert_id: 1,
            fired_at,
            item_id,
            matched_listing_id: None,
            matched_price: Some(100),
            delivered: true,
            delivery_error: None,
            read_at: None,
            title: None,
            body: None,
            click_url: None,
        }
    }

    fn item(id: InboxId, item_id: i32, body: &str, at: DateTime<Utc>, read: bool) -> InboxItem {
        InboxItem {
            id,
            title: "Title".to_string(),
            body: body.to_string(),
            url: None,
            item_id,
            at,
            read,
        }
    }

    fn t(seconds: i64) -> DateTime<Utc> {
        DateTime::<Utc>::UNIX_EPOCH + chrono::Duration::seconds(seconds)
    }

    #[test]
    fn merge_sorts_newest_first_and_is_stable() {
        let server = vec![
            item(InboxId::Server(1), 1, "a", t(10), false),
            item(InboxId::Server(2), 1, "b", t(30), false),
            item(InboxId::Server(3), 1, "c", t(20), false),
        ];
        let merged = merge_inbox(&server, &[]);
        let ids: Vec<_> = merged.into_iter().map(|i| i.id).collect();
        assert_eq!(
            ids,
            vec![InboxId::Server(2), InboxId::Server(3), InboxId::Server(1)]
        );
    }

    #[test]
    fn merge_dedupes_same_server_id() {
        let server = vec![
            item(InboxId::Server(1), 1, "a", t(10), false),
            item(InboxId::Server(1), 1, "a-updated", t(10), true),
        ];
        let merged = merge_inbox(&server, &[]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].body, "a");
    }

    #[test]
    fn unread_count_ignores_read() {
        let items = vec![
            item(InboxId::Server(1), 1, "a", t(1), true),
            item(InboxId::Server(2), 1, "b", t(2), false),
            item(InboxId::Server(3), 1, "c", t(3), false),
        ];
        assert_eq!(unread_count(&items), 2);
    }

    #[test]
    fn push_local_caps_at_100_dropping_oldest() {
        let mut local: Vec<InboxItem> = (0..LOCAL_INBOX_CAP)
            .map(|i| {
                item(
                    InboxId::Local(format!("id-{i}")),
                    1,
                    "b",
                    t(i as i64),
                    false,
                )
            })
            .collect();
        // Oldest entry (pushed first, now at the back) should be dropped.
        let oldest_id = local.last().unwrap().id.clone();
        push_local_bounded(
            &mut local,
            item(InboxId::Local("new".to_string()), 1, "b", t(1000), false),
            LOCAL_INBOX_CAP,
        );
        assert_eq!(local.len(), LOCAL_INBOX_CAP);
        assert_eq!(local[0].id, InboxId::Local("new".to_string()));
        assert!(!local.iter().any(|i| i.id == oldest_id));
    }

    #[test]
    fn mark_read_flips_only_listed_ids() {
        let mut items = vec![
            item(InboxId::Server(1), 1, "a", t(1), false),
            item(InboxId::Server(2), 1, "b", t(2), false),
            item(InboxId::Local("x".to_string()), 1, "c", t(3), false),
        ];
        mark_read(&mut items, &[InboxId::Server(2)]);
        assert!(!items[0].read);
        assert!(items[1].read);
        assert!(!items[2].read);
    }

    #[test]
    fn inbox_item_from_event_falls_back_to_item_name_when_title_missing() {
        let e = event(1, 42, t(1));
        let out = inbox_item_from_event(&e, |id| format!("Item {id}"), |_| "body".to_string());
        assert_eq!(out.title, "Item 42");
        assert_eq!(out.body, "body");
        assert_eq!(out.url, Some("/item/42".to_string()));
        assert!(!out.read);
    }

    #[test]
    fn inbox_item_from_event_uses_click_url_when_present() {
        let mut e = event(1, 42, t(1));
        e.click_url = Some("/some/custom/url".to_string());
        e.title = Some("Custom title".to_string());
        e.read_at = Some(t(2));
        let out = inbox_item_from_event(&e, |id| format!("Item {id}"), |_| "body".to_string());
        assert_eq!(out.title, "Custom title");
        assert_eq!(out.url, Some("/some/custom/url".to_string()));
        assert!(out.read);
    }

    #[test]
    fn merge_collapses_near_duplicate_local_hits() {
        let server = vec![item(InboxId::Server(1), 42, "matched", t(100), false)];
        let local = vec![item(
            InboxId::Local("guest-hit".to_string()),
            42,
            "matched",
            t(101),
            false,
        )];
        let merged = merge_inbox(&server, &local);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, InboxId::Server(1));
    }

    #[test]
    fn merge_keeps_local_hit_when_not_a_near_duplicate() {
        let server = vec![item(InboxId::Server(1), 42, "matched", t(100), false)];
        let local = vec![item(
            InboxId::Local("guest-hit".to_string()),
            43,
            "different item",
            t(101),
            false,
        )];
        let merged = merge_inbox(&server, &local);
        assert_eq!(merged.len(), 2);
    }
}
