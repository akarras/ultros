# Notification infrastructure improvements — design

**Date:** 2026-05-11
**Status:** Approved (high-level), pending implementation plan
**Author:** Claude + Aaron

## Goal

Improve Ultros's notification infrastructure along four user-stated priorities:

1. Streamlined notification UI on the website
2. Better bot commands to manage alerts
3. Web push notifications
4. Shared list notifications

## Current state

- **Alert model is solid.** `alert` + `alert_item_threshold` + `alert_retainer_undercut` define triggers; `notification_endpoint` + `alert_notification_rule` (many-to-many) define delivery; `alert_event` is an audit log.
- **`EndpointConfig` enum is the extension point.** [`ultros/src/alerts/delivery.rs`](../../../ultros/src/alerts/delivery.rs) parses three variants (`DiscordChannel`, `DiscordDm`, `Webhook`) and `dispatch_alert` fans out across all rules for an alert. Adding a fourth (or fifth) variant is the canonical way to add a delivery channel.
- **Frontend is minimal.** [`/alerts`](../../../ultros-frontend/ultros-app/src/routes/alerts.rs) is two tables (rules + recent fires) and [a creation drawer](../../../ultros-frontend/ultros-app/src/components/alert_config_drawer.rs) that re-types the webhook URL every time. Endpoints are not first-class in the UI.
- **Bot has zero price-alert commands.** Retainer/undercut commands exist (`/ffxiv retainer ...`) but price alerts are web-only.
- **Shared lists have no notification touchpoints.** The data model from PR #595 supports sharing but nothing fires when a shared list changes or its items hit a price.
- **Web push is absent.** No `web-push` crate, no VAPID, no service worker, no subscription table.

## Guiding principle

**Lean on the existing endpoint/rule split.** It is already polymorphic over delivery method; every improvement below slots into it rather than building a parallel system. That keeps the audit log (`alert_event`) and dispatch path unified across all channels.

---

## Tier 1: Endpoint-first UI redesign

Pivot `/alerts` from "list of rules" to a three-tab settings hub:

1. **Endpoints** — first-class CRUD. "Add Discord DM" (one-click, uses the invoking user's `discord_user.id`), "Add Discord channel" (channel picker against bot-accessible guilds), "Add webhook URL", and (after Tier 3) "Add this browser". Each endpoint gets a friendly name and a **test** button that invokes the same delivery path with a synthetic payload.
2. **Alert rules** — pick a trigger, pick one-or-more endpoints from a multi-select of saved endpoints. No more re-typing webhook URLs per alert.
3. **History** — `alert_event` exposed as a paginated list with status badges and a **resend** button for failures (which simply calls `dispatch_alert` again with the original payload).

### In-app inbox (SSE)

Add a fifth `EndpointConfig` variant: `InApp { user_id: i64 }`. When `dispatch_alert` hits this branch, it:
1. Inserts a row into a new `notification_inbox` table `(id, user_id, title, body, url, created_at, read_at NULLABLE)`.
2. Pushes the same payload through a per-user `tokio::sync::broadcast` channel held in shared app state. (Single-process assumption — consistent with how Ultros runs today; if the server is ever horizontally scaled this becomes a Redis pub/sub or similar.)

A new SSE endpoint `/api/v1/notifications/stream` subscribes the current session to the user's broadcast channel. The navbar gains a bell icon with unread count (polled or streamed).

This trades a tiny bit of schema purity (an "InApp endpoint" is not really external) for unified history and zero duplicated dispatch logic.

---

## Tier 2: Bot commands

Mirror the web surface as `/alert` subcommands under the existing `/ffxiv` group (where retainer/list commands already live):

| Command | Effect |
|---|---|
| `/ffxiv alert price <item> <price> [hq] [world]` | Create a `BelowThreshold` alert. Auto-creates a `DiscordDm` endpoint for the invoking user if none exists. |
| `/ffxiv alert here` | Add the current channel as a `DiscordChannel` endpoint and present a button menu to bind it to existing alerts. |
| `/ffxiv alert list` | Paginated embed of the user's rules with **mute** buttons. |
| `/ffxiv alert mute <id> [duration]` | Sets `alert.enabled = false` or a temporary `muted_until` (new column, nullable). |
| `/ffxiv alert unmute <id>` | Clears mute state. |
| `/ffxiv alert remove <id>` | Confirm-then-delete. |
| `/ffxiv endpoint list` | List user's endpoints with method, name, last-fired. |
| `/ffxiv endpoint remove <id>` | Delete an endpoint (cascades to `alert_notification_rule`). |

**Implementation note:** every command writes through `UltrosDb` only — no parallel command-side data model. The "auto-create DM endpoint if missing" idempotency check belongs in the DB layer (`UltrosDb::get_or_create_dm_endpoint(user_id)`).

---

## Tier 3: Web push

Build order, with each step independently testable:

1. **Crate + keys.** Add the `web-push` crate. VAPID keys go in env vars (`VAPID_PUBLIC_KEY`, `VAPID_PRIVATE_KEY`, `VAPID_CONTACT_EMAIL`); public key is served via `GET /api/v1/push/vapid-public-key`.
2. **Migration: `push_subscription` table.** Columns: `(id PK, user_id FK → discord_user, endpoint TEXT, p256dh TEXT, auth TEXT, user_agent TEXT NULL, created_at, last_seen_at)`. Unique on `(user_id, endpoint)`.
3. **Endpoint variant.** Add `EndpointConfig::WebPush { subscription_id: i32 }` to delivery.rs. `send_webpush(subscription_id, title, body, db)`:
   - Looks up the subscription.
   - Builds a `WebPushMessage` with the `web-push` crate's `aes128gcm` content encoding.
   - `POST`s with the crate's `IsahcWebPushClient` (or `reqwest` equivalent).
   - On `410 Gone` / `404 Not Found`: soft-delete the subscription, mark the `alert_event` with a specific delivery_error, and surface in the history UI.
4. **Service worker.** New file at site root `/service-worker.js` with `push` and `notificationclick` handlers. The `push` handler calls `self.registration.showNotification(title, {body, data:{url}})`; the `notificationclick` handler opens or focuses `data.url`.
5. **Frontend subscribe flow.** In the Endpoints tab, "Enable browser notifications" button:
   - `Notification.requestPermission()`
   - `navigator.serviceWorker.register('/service-worker.js')`
   - `registration.pushManager.subscribe({userVisibleOnly: true, applicationServerKey: <vapid-public>})`
   - `POST /api/v1/push/subscribe` with `{endpoint, keys: {p256dh, auth}, user_agent}` — server creates both the `push_subscription` row *and* a `notification_endpoint` row of method `WebPush` whose `config` is `{"subscription_id": N}`.
6. **Payload.** `{title, body, url}` JSON, kept under 4KB to stay within push-service limits.

**Rationale for splitting `push_subscription` from `notification_endpoint`:** the subscription holds cryptographic material (p256dh, auth) and a URL that the browser regenerates; the endpoint holds the user-facing name and the rule bindings. Keeping them separate keeps crypto out of the generic `config` JSON and lets us refresh subscriptions without touching alert rules.

---

## Tier 4: Shared list notifications

Two user-visible features, one mechanism.

### List-scoped price alerts

> "Notify me when **any** item in [shopping list X] drops below its target price."

- New trigger: `AlertTrigger::ListItemThreshold { list_id: i32, per_item: bool }`.
- Schema additions:
  - `list_item.target_price BIGINT NULL` (per-item override; `NULL` means "use list default").
  - `list.default_target_price_pct INT NULL` (optional: "alert if X% below market"; deferred unless needed).
- Trigger evaluation: when the price tracker emits a listing for an item in any list with active `ListItemThreshold` alerts, check the listing price against `list_item.target_price`. On match, fire the alert.
- **Fan-out:** the alert is owned by a single user (the subscriber), not the list. Each member who wants notifications creates their own alert against the list. This avoids cross-user permission surprises and keeps the existing 1-alert-N-endpoints model.

### List-change events

Lower priority; included for completeness:

- New table `list_event (id, list_id, kind TEXT, actor_user_id, payload JSONB, created_at)`.
- New trigger `AlertTrigger::ListChange { list_id, events: Vec<ListEventKind> }`.
- Kinds: `ItemAdded`, `ItemRemoved`, `MemberAdded`, `MemberRemoved`, `ListRenamed`.
- Fired synchronously from the existing list-mutation endpoints (insert into `list_event`, then nudge the alert dispatcher).

### Permission gate

Subscribing to a list (i.e. creating a `ListItemThreshold` or `ListChange` alert) requires `UltrosDb::can_view_list(user_id, list_id) >= View`. The check belongs at the alert-creation endpoint, not at dispatch time — once subscribed, dispatch trusts the subscription. If a user's share is revoked, the next dispatch should re-check and soft-disable the alert.

---

## Build sequence

1. **Endpoint-first UI + history surface** (Tier 1, no schema changes). ~1 day.
2. **Bot commands** (Tier 2, additive). ~1 day.
3. **In-app SSE inbox** (Tier 1 finalizer, one new `EndpointConfig` variant + one table). ~half day.
4. **Web push** (Tier 3). ~2-3 days.
5. **Shared list notifications** (Tier 4, depends on Tiers 1+3). ~2 days.

## Out of scope (deliberately)

- **Separate notifications microservice** — volume doesn't justify; existing tokio listeners are fine.
- **Email delivery** — no email collection today; would mean adding SMTP for no asked-for need.
- **Mobile push (FCM/APNs)** — web push covers modern mobile browsers including iOS 16.4+ Safari PWAs. Skip native until demand exists.

## Open judgment calls (documented for posterity)

- **Reuse the alert framework for shared-list events** rather than a parallel "list subscription" pipeline. Costs: stretches what "alert" means. Gains: one audit log, one dispatch path.
- **In-app SSE inbox as an `EndpointConfig` variant.** Same trade.
- **`push_subscription` split from `notification_endpoint`.** Cleaner separation of crypto material from rule metadata; costs one join.
- **`/ffxiv alert` rather than `/alert`.** Stays under existing command tree; consistent with `/ffxiv retainer`, `/ffxiv list`.

## Risks

- **Service worker scope** — Leptos hydration must not race with the SW registration. Mitigation: register the SW lazily, only after the user clicks "enable browser notifications", not on every page load.
- **VAPID key rotation** — keys are effectively permanent in this scheme; rotating them invalidates every existing subscription. Document this; do not rotate without a forced-resubscribe migration path.
- **Push payload encryption** — handled by the `web-push` crate, but version pins matter; the crate has had API churn. Pin to a known-good version and call it out in CI.
- **Shared list permission drift** — a user with a now-revoked share could still have an active alert. Mitigation above (re-check at dispatch, soft-disable on failure).
