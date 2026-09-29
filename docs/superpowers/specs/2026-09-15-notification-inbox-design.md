# Notification inbox + guest (auth-less) price alerts

## Context

Ultros alerts today fire only outward: Discord DM/channel, webhook, or web push. The site
itself never tells you an alert fired; `/alerts` › History is a 50-row table with no
read state. Every alert route hard-requires the Discord cookie, so a visitor who has not
signed in cannot be notified of anything. The 2026-05-11 notification-infrastructure
spec sketched an in-app inbox as a "Tier 1 finalizer"; it was never built.

The owner wants (1) a **notification inbox at the bottom of the sidebar** with an unread
badge, and (2) **auth-less alerts** that work "as long as the browser is open".

### Premise correction (confirmed during exploration)

There is **no server-side device identity**. The Lists 2.0 "device ID" is a per-list
`device:<uuid>` record key in IndexedDB (`ultros/static/guest-list-store.mjs`); the server
only sees it as a dedupe string inside `POST /api/v1/list/adopt`, and the product spec
states "possessing a device ID is not authorization". The only server principal is
`AuthDiscordUser`. Guest alerts therefore cannot piggyback on a device principal.

### Decisions made with the owner

| Question | Decision |
|---|---|
| Guest alert model | **Client-evaluated.** Rules live in the browser; the tab subscribes to the existing anonymous-tolerant realtime websocket for Listings on those items and evaluates thresholds in-browser. No server identity. Adopts into account alerts on sign-in (mirrors device-list adoption). |
| Signed-in feed | **Websocket push + server read state.** New `SubscribeNotifications` on `/api/v1/realtime/events`; `alert_event.read_at` + mark-read endpoint so unread agrees across devices. |
| Guest alert kinds (v1) | **Item price threshold only** (same shape as `AlertTrigger::BelowThreshold`) so it adopts 1:1. |

### Gaps found in the notification system

1. **No in-app channel.** Fires reach Discord/webhook/push only. *(fixed here)*
2. **No read/unread state** on `alert_event`; History hardcoded to 50 rows, no cursor. *(fixed here)*
3. **Alerts hard-require login**; guests can't even use web push (`notification_endpoint.user_id` is required). *(guest path added here; guest web push stays out)*
4. **Fires are not pushed over the realtime socket.** The legacy `/alerts/websocket` is undercut-only. *(fixed here)*
5. **An alert with zero deliverable endpoints never advances its cooldown** (`update_alert_last_fired` runs only on Delivered), so it re-fires on every listing event and logs `PermanentFailure` each time. *(fixed here via the `InApp` endpoint)*
6. **Undercut and retainer-sold trackers ignore `cooldown_seconds`** (only threshold/list trackers call `is_off_cooldown_at`). *Not fixed; follow-up.*
7. **`alert_event` stores no message text**; every consumer re-derives wording. *(fixed here: title/body/click_url persisted)*
8. **Server alert text is English only.** Accepted for v1 (Discord text already is); inbox rows lead with the localized item name.
9. **Legacy tables** `alert_price`, `alert_discord_destination` are vestigial. *Not touched.*
10. **No item-page entry point** to create a price alert; and device/guest list rows have **no bell** (`BuildListRow` in `list_view_sync.rs` lacks it; only account `ListItemRow` has one). *Follow-ups.*
11. **`alert.owner` has no index** (FK only since 2022); every per-user alert query scans. *(index added here)*

## Architecture

Server: every fire already writes an `alert_event`; add `read_at/title/body/click_url`,
broadcast the inserted row on a new `notifications` bus, and let an authenticated socket
subscribe to its owner's events. A no-op `InApp` endpoint ("This site") makes inbox-only
alerts count as Delivered. Client: one root-provided `Inbox` store merges (a) server events
(initial GET + live socket) and (b) local guest hits produced by a hydrate-only
`GuestAlertEvaluator` that evaluates browser-stored rules against Listings events with the
same pure predicate the server uses. The sidebar gets a bottom inbox row with a drop-up
panel and an unread count pill.

**Delivery as two PRs** (stacked PRs get zero CI in this repo, so land sequentially):
- **PR A — backend + wire types** (Tasks A1–A11). Compiles the whole workspace on its own
  (includes the two exhaustive-match arms the frontend needs).
- **PR B — frontend** (Tasks B0–B12), based on merged main.

### Cross-PR contracts (pin these exactly)

```rust
// ultros-api-types/src/alert.rs
pub struct AlertEvent { ...existing..., #[serde(default)] read_at: Option<DateTime<Utc>>,
    #[serde(default)] title: Option<String>, #[serde(default)] body: Option<String>,
    #[serde(default)] click_url: Option<String> }
pub struct MarkAlertEventsReadRequest { #[serde(default)] ids: Vec<i64>, #[serde(default)] up_to_id: Option<i64> }
pub struct MarkAlertEventsReadResponse { updated: u64, unread_count: u64 }
pub struct UnreadAlertEventCount { unread: u64 }
pub enum EndpointMethod { ..., InApp {} }          // wire: {"method":"InApp"}
pub struct ThresholdRule { item_id, world_selector: AnySelector, price_threshold, hq_only, cooldown_seconds, last_fired_at: Option<DateTime<Utc>> }
pub fn is_off_cooldown_at(last: Option<DateTime<Utc>>, cooldown_seconds: i32, now: DateTime<Utc>) -> bool
pub fn threshold_listing_matches(rule: &ThresholdRule, listing: &ActiveListing, worlds: &WorldHelper, now: DateTime<Utc>) -> bool
// ultros-api-types/src/websocket.rs
ClientMessage::SubscribeNotifications { #[serde(default)] subscription_id: Option<u64> }
ServerClient::Notification(AlertEvent)   // ALWAYS sent wrapped in SubscriptionEvent
// REST
GET  /api/v1/alerts/events?limit=<1..200, default 50>&before_id=<i64>   // ordered by id DESC
POST /api/v1/alerts/events/read   (MarkAlertEventsReadRequest -> MarkAlertEventsReadResponse)
GET  /api/v1/alerts/events/unread_count
GET  /api/v1/endpoints            // always includes the caller's InApp endpoint (auto-created)
```

---

